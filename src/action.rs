use mysql::{prelude::Queryable, Conn, Opts, Row};
use pipelined_server::{
    http::{
        body::{Application, Body, ContentType, Text},
        request::{method::Method, parser_error::ParserError, Request},
        response::{response_status_code::ResponseStatusCode, Response},
    },
    pipeline::default::action::{default_err_page, not_allowed_logic},
    setting::ServerSetting,
    ActionBuilder,
};

use hex_rgb::Color;
use petgraph::{adj::NodeIndex, Directed, Direction::Outgoing, Graph};
use pulldown_cmark::{html, Options, Parser};
use regex::Regex;
use serde_json::Value;

use std::{
    collections::{HashMap, VecDeque},
    env,
    fs::read,
    path::PathBuf,
    sync::mpsc::{self, Sender},
    thread::{self, JoinHandle},
};

use log::{error, trace};
#[derive(Clone, Debug)]
pub enum UtilityCommand {
    GetFile {
        file: PathBuf,
        bytes: bool,
    },
    DBQuery {
        statement: String,
        param: Vec<String>,
        map_func: fn(Row) -> String,
    },
}

#[derive(Clone, Debug)]
pub enum UtilityData {
    Bytes(Vec<u8>),
    String(Vec<String>),
}

const SELECT_SKILLS: &str = "SELECT * FROM skills";
const SELECT_PROJECTS: &str = "select `tag`.`id` AS `id`,`tag`.`colour` AS `colour`,`tag`.`tag_name` AS `tag_name`,`dev_log`.`body` AS `body`,`project_details`.`repo` AS  `repo`,`project_details`.`first_push` AS `first_push`,`project_details`.`last_push` AS `last_push` from (((`tag` join `project_details`) join `dev_log`) join `relate_tags`) where ((`tag`.`tag_type` = 1) and (`tag`.`id` = `project_details`.`proj_tag`) and (`relate_tags`.`tag_1` = `project_details`.`proj_tag`) and (`relate_tags`.`tag_2` = `dev_log`.`tag_id`))";
const SELECT_TAGS: &str = "SELECT `id`,`colour`,`tag_name`,`tag_type` FROM tag WHERE tag_type!=3";
const SELECT_RELATED: &str = "SELECT `tag_1`,`tag_2` FROM relate_tags";

const NULL: &str = "null";

type RGB = (f64, f64, f64);

pub type UtilitySender = mpsc::Sender<(UtilityCommand, Sender<Result<UtilityData, ()>>)>;

pub fn generate_utility_thread() -> (UtilitySender, JoinHandle<()>) {
    // generate channel
    let (tx, rx): (UtilitySender, _) = mpsc::channel();

    // create thread
    let thread = thread::spawn(move || {
        let mut db_request_next: VecDeque<(
            String,
            Vec<String>,
            fn(Row) -> String,
            Sender<Result<UtilityData, ()>>,
        )> = VecDeque::new();
        let mut db_request_pool: Vec<JoinHandle<()>> = Vec::new();

        let mut file_request_next: VecDeque<(PathBuf, bool, Sender<Result<UtilityData, ()>>)> =
            VecDeque::new();
        let mut file_request_pool: Vec<JoinHandle<()>> = Vec::new();

        loop {
            if let Ok((utility_command, sender)) = rx.try_recv() {
                trace!("Cmd: {utility_command:?}");
                match utility_command {
                    UtilityCommand::DBQuery {
                        statement,
                        param,
                        map_func,
                    } => db_request_next.push_back((statement, param, map_func, sender)),
                    UtilityCommand::GetFile { file, bytes } => {
                        file_request_next.push_back((file, bytes, sender))
                    }
                }
            }

            if db_request_pool.len() < 4 && !db_request_next.is_empty() {
                let (statement, params, map_func, sender) = db_request_next.pop_front().unwrap();

                db_request_pool.push(thread::spawn(move || {
                    let url = {
                        let host = env::var("DB_host").unwrap();
                        let name = env::var("DB_name").unwrap();
                        let user_name = env::var("DB_username").unwrap();
                        let password = env::var("DB_password").unwrap();
                        let port = env::var("DB_port").unwrap();

                        format!("mysql://{user_name}:{password}@{host}:{port}/{name}")
                    };

                    let url = match Opts::from_url(&url) {
                        Ok(url) => url,
                        Err(err) => {
                            error!("Failed to get url: {err}");
                            let _ = sender.send(Err(()));
                            return;
                        }
                    };

                    let mut conn = match Conn::new(url) {
                        Ok(url) => url,
                        Err(err) => {
                            error!("Failed to conn: {err}");
                            let _ = sender.send(Err(()));
                            return;
                        }
                    };

                    let statement = match conn.prep(statement) {
                        Ok(statement) => statement,
                        Err(err) => {
                            error!("Failed to prep: {err}");
                            let _ = sender.send(Err(()));
                            return;
                        }
                    };

                    let rows = match conn.exec_iter(statement, params) {
                        Ok(val) => val,
                        Err(err) => {
                            error!("Failed to execute: {err:#?}");
                            let _ = sender.send(Err(()));
                            return;
                        }
                    };

                    let mut results = Vec::new();

                    for row in rows {
                        match row {
                            Ok(row) => results.push(map_func(row)),
                            Err(err) => {
                                error!("Failed to parse data: {err:#?}");
                                let _ = sender.send(Err(()));
                                return;
                            }
                        };
                    }

                    let _ = sender.send(Ok(UtilityData::String(results)));

                    // let _ = match conn.exec_map(statement, params, map_func) {
                    //     Ok(val) => sender.send(Ok(UtilityData::String(val))),
                    //     Err(err) => {
                    //         error!("Failed to execute: {err:#?}");
                    //         sender.send(Err(()))
                    //     },
                    // };
                }));
            }

            if file_request_pool.len() < 4 && !file_request_next.is_empty() {
                let (path, byte_cond, sender) = file_request_next.pop_front().unwrap();

                file_request_pool.push(thread::spawn(move || {
                    let path = path.as_path();

                    //check if file exists
                    if !path.exists() || !path.is_file() {
                        let _ = sender.send(Err(()));
                        return;
                    }

                    let Some(ext) = path.extension().unwrap().to_str() else {
                        let _ = sender.send(Err(()));
                        return;
                    };

                    match (ext, byte_cond) {
                        ("txt", false) | ("html", false) | ("css", false) | ("template", false) => {
                            let _ = sender.send(Ok(UtilityData::String({
                                let tmp = String::from_utf8(read(path).unwrap()).unwrap();
                                vec![tmp]
                            })));
                        }
                        (_, true) => {
                            let _ = sender.send(Ok(UtilityData::Bytes(read(path).unwrap())));
                        }
                        _ => {
                            let _ = sender.send(Err(()));
                        }
                    }
                }));
            }

            //filter our completed threads
            db_request_pool = db_request_pool
                .into_iter()
                .filter(|t| !t.is_finished())
                .collect();
            file_request_pool = file_request_pool
                .into_iter()
                .filter(|t| !t.is_finished())
                .collect();
        }
    });

    (tx, thread)
}

pub fn post(
    request: &Request,
    _setting: &ServerSetting,
    utility_thread: &UtilitySender,
) -> Result<Response, ResponseStatusCode> {
    let Request(method, _heading) = request;

    let Method::Post { file, body } = method else {
        panic!();
    };

    // let Some(host) = heading.get("host") else {
    //     return Err(ResponseStatusCode::ImATeapot);
    // };

    let request = file.trim_matches('/').trim_matches('\\');

    match request {
        "get_data" => {
            let Ok(body) = parse_json(body) else {
                return Err(ResponseStatusCode::BadRequest);
            };

            let requested_data: Vec<Value>;

            if body.is_array() {
                requested_data = body.as_array().unwrap().to_vec();
            } else if body.is_string() {
                requested_data = vec![body];
            } else {
                return Err(ResponseStatusCode::BadRequest);
            }

            let mut data = requested_data
                .iter()
                .filter(|table_name| table_name.is_string())
                .filter_map(|table_name| table_name.as_str())
                .filter_map(|table_name| {
                    let (tx, rx) = mpsc::channel();
                    let _ = match table_name {
                        "skills" => utility_thread.send((
                            UtilityCommand::DBQuery {
                                statement: String::from(SELECT_SKILLS),
                                param: Vec::new(),
                                map_func: parse_db_skills,
                            },
                            tx,
                        )),
                        "projects" => utility_thread.send((
                            UtilityCommand::DBQuery {
                                statement: String::from(SELECT_PROJECTS),
                                param: Vec::new(),
                                map_func: parse_db_projects,
                            },
                            tx,
                        )),
                        "tag" => utility_thread.send((
                            UtilityCommand::DBQuery {
                                statement: String::from(SELECT_TAGS),
                                param: Vec::new(),
                                map_func: parse_db_tags,
                            },
                            tx,
                        )),
                        "related" => utility_thread.send((
                            UtilityCommand::DBQuery {
                                statement: String::from(SELECT_RELATED),
                                param: Vec::new(),
                                map_func: parse_db_related,
                            },
                            tx,
                        )),
                        _ => return None,
                    };

                    let data = match rx.recv() {
                        Ok(Ok(UtilityData::String(data))) => data,
                        Ok(Ok(UtilityData::Bytes(_))) => return Some(Err(())),
                        Ok(Err(_)) => {
                            error!("Failed utility thread");
                            return Some(Err(()));
                        }
                        Err(err) => {
                            error!("Failed Channel: {err}");
                            return Some(Err(()));
                        }
                    };

                    return Some(Ok(format!(
                        "{{\"name\":\"{}\",\"data\":[{}]}}",
                        table_name,
                        data.join(",")
                    )));
                });

            // if data.any(|data| data.is_err()) {
            //     return Err(ResponseStatusCode::InternalServerError);
            // }

            let data = data
                .into_iter()
                .filter(|result| result.is_ok())
                .map(|data| data.unwrap())
                .collect::<Vec<String>>();

            trace!("{data:#?}");

            return Ok(Response {
                status: ResponseStatusCode::Ok,
                header: HashMap::new(),
                body: Some(Body {
                    content_type: ContentType::Application(Application::json),
                    content: format!("{:?}", data).as_bytes().to_vec(),
                }),
            });
        }
        _ => Err(ResponseStatusCode::BadRequest),
    }
}

fn parse_db_skills(row: Row) -> String {
    let id = match row.get::<i32, usize>(0) {
        Some(val) => val,
        None => return String::from(""),
    };
    let colour: String = match row.get::<String, usize>(1) {
        Some(val) => val,
        None => return String::from(""),
    };
    let symbol: String = match row.get::<String, usize>(2) {
        Some(val) => val,
        None => return String::from(""),
    };

    return format!(
        "{{\"id\":{}, \"colour\":\"#{}\", \"symbol\":\"{}\"}}",
        id, colour, symbol
    );
}

fn parse_db_projects(row: Row) -> String {
    let id = match row.get::<i32, usize>(0) {
        Some(val) => val,
        None => return String::from(""),
    };
    let _colour = match row.get::<String, usize>(1) {
        Some(val) => val,
        None => return String::from(""),
    };
    let name = match row.get::<String, usize>(2) {
        Some(val) => val,
        None => return String::from(""),
    };
    let body = match row.get::<String, usize>(3) {
        Some(val) => val,
        None => return String::from(""),
    };
    let repo = match row.get::<String, usize>(4) {
        Some(val) => val,
        None => return String::from(""),
    };
    let first_push = match row.get::<mysql::Value, usize>(5) {
        Some(mysql::Value::Date(year, month, day, _hour, _min, _sec, _ms)) => {
            format!("{year}-{month}-{day}")
        }
        Some(_) | None => return String::from(""),
    };
    let last_push = match row.get::<Option<mysql::Value>, usize>(6) {
        Some(Some(mysql::Value::Date(year, month, day, _hour, _min, _sec, _ms))) => {
            format!("{year}-{month}-{day}")
        }
        Some(None) => String::from(NULL),
        Some(Some(_)) | None => return String::from(""),
    };

    return format!(
        "{{\"Tag\": {id}, \"Start\": \"{first_push}\", \"Update\": \"{last_push}\", \"Title\":\"{name}\", \"Description\": \"{body}\", \"link\":\"{repo}\"}}",
    );
}

fn parse_db_tags(row: Row) -> String {
    let id: i32 = match row.get::<i32, usize>(0) {
        Some(val) => val,
        None => return String::from(""),
    };

    let colour: String = match row.get::<String, usize>(1) {
        Some(val) => val,
        None => return String::from(""),
    };

    let symbol: String = match row.get::<String, usize>(2) {
        Some(val) => val,
        None => return String::from(""),
    };

    let tag_type: i32 = match row.get::<i32, usize>(3) {
        Some(val) => val,
        None => return String::from(""),
    };

    return format!(
        "{{\"id\":{}, \"colour\":\"#{}\", \"symbol\":\"{}\", \"tag_type\":{}}}",
        id, colour, symbol, tag_type
    );
}

fn parse_db_related(row: Row) -> String {
    let tag_1: i32 = match row.get::<i32, usize>(0) {
        Some(val) => val,
        None => return String::from(""),
    };

    let tag_2: i32 = match row.get::<i32, usize>(1) {
        Some(val) => val,
        None => return String::from(""),
    };

    return format!("{{\"tag_1\":{}, \"tag_2\":{}}}", tag_1, tag_2);
}

fn parse_json(body: &Body) -> Result<serde_json::Value, ParserError> {
    match &body.content_type {
        ContentType::Application(value) => match value {
            Application::json => {
                let content = String::from_utf8_lossy(&body.content);

                match serde_json::from_str(&content) {
                    Ok(ok) => Ok(ok),
                    Err(err) => {
                        return Err(ParserError::InvalidMethod(Some(format!(
                            "failed to parse json: {}",
                            err
                        ))))
                    }
                }
            }
            _ => {
                return Err(ParserError::InvalidMethod(Some(format!(
                    "Invalid Content Type variant. Got {} instead",
                    &body.content_type.to_string()
                ))))
            }
        },
        _ => {
            return Err(ParserError::InvalidMethod(Some(format!(
                "Invalid Content Type. Got {} instead",
                &body.content_type.to_string()
            ))))
        }
    }
}

pub fn get(
    request: &Request,
    setting: &ServerSetting,
    utility_thread: &UtilitySender,
) -> Result<Response, ResponseStatusCode> {
    trace!("{request:#?}");
    let Request(method, heading) = request;

    let Method::Get { file } = method else{
        panic!()
    };

    let Some(host) = heading.get("host") else {
        return Err(ResponseStatusCode::ImATeapot);
    };

    let Some((
        host_path,
        allowed_extension
    )) = setting.paths.get(host).map(|host_path| (&host_path.path, &host_path.allow)) else {
        return Err(ResponseStatusCode::NotFound);
    };

    let file = {
        let ext: &str;
        let mut file_path = PathBuf::from(&host_path);
        let file = file.trim_matches('/').replace("/", "\\");
        file_path.push(&file);
        if file_path.extension().is_none() {
            file_path.push("index.html");
        }
        ext = file_path.extension().unwrap().to_str().unwrap();

        if !allowed_extension.iter().any(|allowed| allowed == ext) {
            return Err(ResponseStatusCode::Forbidden);
        }

        file_path
    };

    trace!("file:{:?}", file);

    // check file exists
    {
        let (tx, rx) = mpsc::channel();

        let _ = utility_thread.send((
            UtilityCommand::GetFile {
                file: file.clone(),
                bytes: true,
            },
            tx,
        ));

        if let Ok(Ok(file_content)) = rx.recv() {
            let header = {
                let mut header = HashMap::new();

                header
            };
            let content = match file_content {
                UtilityData::Bytes(byte) => byte,
                UtilityData::String(data) => data
                    .into_iter()
                    .map(|content| content.as_bytes().to_vec())
                    .fold(Vec::new(), |mut bytes, content| {
                        bytes.append(&mut content.into());

                        bytes
                    }),
            };

            return Ok(Response {
                status: ResponseStatusCode::Ok,
                header: header,
                body: Some(Body {
                    content_type: ContentType::try_from(
                        file.extension().unwrap().to_str().unwrap(),
                    )
                    .unwrap(),
                    content,
                }),
            });
        }
    }

    // get template & db value
    {
        let (tx, db_rx) = mpsc::channel();
        let _ = utility_thread.send((
            UtilityCommand::DBQuery {
                statement: format!("select * from ({SELECT_PROJECTS}) as p"),
                param: vec![],
                map_func: |row: Row| {
                    let id = match row.get::<i32, usize>(0) {
                        Some(val) => val,
                        None => return String::from(""),
                    };
                    let _colour = match row.get::<String, usize>(1) {
                        Some(val) => val,
                        None => return String::from(""),
                    };
                    let name = match row.get::<String, usize>(2) {
                        Some(val) => val,
                        None => return String::from(""),
                    };
                    let body = match row.get::<String, usize>(3) {
                        Some(val) => val,
                        None => return String::from(""),
                    };
                    let repo = match row.get::<String, usize>(4) {
                        Some(val) => val,
                        None => return String::from(""),
                    };
                    let first_push = match row.get::<mysql::Value, usize>(5) {
                        Some(mysql::Value::Date(year, month, day, _hour, _min, _sec, _ms)) => {
                            format!("{year}-{month}-{day}")
                        }
                        Some(_) | None => return String::from(""),
                    };
                    let last_push = match row.get::<Option<mysql::Value>, usize>(6) {
                        Some(Some(mysql::Value::Date(
                            year,
                            month,
                            day,
                            _hour,
                            _min,
                            _sec,
                            _ms,
                        ))) => format!("{year}-{month}-{day}"),
                        Some(None) => String::from(NULL),
                        Some(Some(_)) | None => return String::from(""),
                    };

                    return format!("{id}, {first_push}, {last_push}, {name}, {body}, {repo}",);
                },
            },
            tx,
        ));

        match db_rx.recv() {
            Ok(Ok(UtilityData::String(data))) => {
                let proj = data
                    .iter()
                    .map(|proj_raw| {
                        let proj_raw = proj_raw
                            .split(",")
                            .map(|str| String::from(str.trim()))
                            .collect::<Vec<String>>();

                        // "{id}, {first_push}, {last_push}, {name}, {body}, {repo}",
                        (
                            proj_raw[0].clone(),
                            proj_raw[1].clone(),
                            proj_raw[2].clone(),
                            proj_raw[3].clone(),
                            proj_raw[4].clone(),
                            proj_raw[5].clone(),
                        )
                    })
                    .filter(|proj| {
                        format!(
                            "\"{}\\\\{}\\\\index.html\"",
                            host_path.replace("\\", "\\\\"),
                            proj.3.replace(" ", "_")
                        ) == format!("{file:?}")
                    })
                    .next();

                if let Some(proj) = proj {
                    //query data async
                    //get template files
                    let (tx, dev_log_article_template_rx) = mpsc::channel();
                    let _ = utility_thread.send((
                        UtilityCommand::GetFile {
                            file: {
                                let mut file_path = PathBuf::from(host_path);
                                file_path.push("src");
                                file_path.push("template");
                                file_path.push("dev_log.article.html.template");

                                file_path
                            },
                            bytes: false,
                        },
                        tx,
                    ));

                    let (tx, dev_log_template_rx) = mpsc::channel();
                    let _ = utility_thread.send((
                        UtilityCommand::GetFile {
                            file: {
                                let mut file_path = PathBuf::from(host_path);
                                file_path.push("src");
                                file_path.push("template");
                                file_path.push("dev_log.html.template");

                                file_path
                            },
                            bytes: false,
                        },
                        tx,
                    ));

                    //query db
                    let (tx, related_tags_rx) = mpsc::channel();
                    let _ = utility_thread.send(
                        (
                            UtilityCommand::DBQuery {
                                statement: String::from(
                                    "SELECT id, colour, tag_name FROM tag WHERE EXISTS(SELECT * FROM relate_tags WHERE relate_tags.tag_1 = ? AND relate_tags.tag_2 = tag.id) AND tag_type != 3"
                                ),
                                param: vec![proj.0.clone()],
                                map_func: |row| {
                                    //id,colour,tag_name
                                    let id = row.get::<i32, usize>(0).unwrap();
                                    let colour = row.get::<String, usize>(1).unwrap();
                                    let tag_name = row.get::<String, usize>(2).unwrap();

                                    format!("{id},{colour},{tag_name}")
                                }
                            },
                            tx
                        )
                    );
                    let (tx, dev_log_chain_rx) = mpsc::channel();
                    let _ = utility_thread.send(
                        (
                            UtilityCommand::DBQuery {
                                statement: String::from(
                                    "SELECT Distinct tag_1, tag_2 FROM relate_tags, tag WHERE (tag_1 = id AND tag_type = 3) OR (tag_2 = id AND tag_type = 3)"
                                ),
                                param: vec![],
                                map_func: |row| {
                                    //tag_1,tag_2
                                    let tag_1 = row.get::<i32, usize>(0).unwrap();
                                    let tag_2 = row.get::<i32, usize>(1).unwrap();

                                    format!("{tag_1},{tag_2}")
                                }
                            },
                            tx
                        )
                    );
                    let (tx, dev_log_rx) = mpsc::channel();
                    let _ = utility_thread.send(
                        (
                            UtilityCommand::DBQuery {
                                statement: String::from(
                                    "SELECT id, tag_name, created, body FROM dev_log, tag WHERE id=tag_id"
                                ),
                                param: vec![],
                                map_func: |row| {
                                    //id,tag_name,created,body
                                    let id = row.get::<i32, usize>(0).unwrap();
                                    let name = row.get::<String, usize>(1).unwrap();
                                    let created = match row.get::<Option<mysql::Value>, usize>(2).unwrap() {
                                        Some(mysql::Value::Date(year, month, day, _, _, _, _)) => format!("{year}-{month}-{day}"),
                                        Some(_) => panic!(),
                                        None => String::from(NULL),
                                    };
                                    let body = row.get::<String, usize>(3).unwrap();

                                    format!("{id},{name},{created},{body}")
                                }
                            },
                            tx
                        )
                    );

                    //initialize dev log page
                    let file_content = match dev_log_template_rx.recv() {
                        Ok(Ok(UtilityData::String(template))) => template[0].clone(),
                        Err(err) => {
                            error!("{err:?}");
                            return Err(ResponseStatusCode::InternalServerError);
                        }
                        _ => return Err(ResponseStatusCode::InternalServerError),
                    };

                    // "{id}, {first_push}, {last_push}, {name}, {body}, {repo}"
                    let regex: Regex = Regex::new("\\{start\\}").unwrap();
                    let file_content = regex.replace_all(&file_content, proj.1);
                    let regex: Regex = Regex::new("\\{update\\}").unwrap();
                    let file_content = regex.replace_all(&file_content, proj.2);

                    let regex: Regex = Regex::new("\\{project_name\\}").unwrap();
                    let file_content = regex.replace_all(&file_content, proj.3);

                    let regex: Regex = Regex::new("\\{summary\\}").unwrap();
                    let file_content = regex.replace_all(&file_content, proj.4);

                    let regex: Regex = Regex::new("\\{link\\}").unwrap();
                    let file_content = regex.replace_all(&file_content, proj.5);

                    let regex: Regex = Regex::new("\\{tags\\}").unwrap();
                    let file_content = regex.replace_all(&file_content, {
                        let data = match related_tags_rx.recv() {
                            Ok(Ok(UtilityData::String(related))) => related,
                            Err(err) => {
                                error!("{err:?}");
                                return Err(ResponseStatusCode::InternalServerError)
                            },
                            _ => return Err(ResponseStatusCode::InternalServerError),
                        };

                        data.iter()
                            //convert Vec<String> -> Iter<Vec<String>>
                            .map(|row| {
                                return row.split(",")
                                    .map(|val| String::from(val.trim()))
                                    .collect::<Vec<String>>()
                            })
                            //convert Iter<String> -> Iter<(String, (RGB, RGB), String)>
                            .map(
                            |row| {
                                //id,colour,tag_name
                                let colour = row[1].clone();
                                let border_colour = Color::new(&format!("#{}", colour)).unwrap();
                                let border_colour = (border_colour.red as f64, border_colour.green as f64, border_colour.blue as f64);
                                let bg_colour = (
                                    calculate_colour(border_colour.0, 0f64, 0.75f64),
                                    calculate_colour(border_colour.1, 0f64, 0.75f64),
                                    calculate_colour(border_colour.2, 0f64, 0.75f64),
                                );

                                (row[0].clone(), (border_colour, bg_colour), row[2].clone())
                            }
                        )
                            //convert Iter<(String, (RGB, RGB), String)> -> Iter<String> #html string
                            .map(|(id,(border_colour, background_colour),tag_name)| {
                                format!(
                                    "<div id=\"{id}\" class=\"tag\" style=\"border-color:rgb({},{},{});background-color:rgb({},{},{});\">{tag_name}</div>",
                                    border_colour.0, border_colour.1, border_colour.2,
                                    background_colour.0, background_colour.1, background_colour.2,
                            )
                            })
                            // Iter<String> -> String
                            .fold(
                                String::new(),
                                |mut tag_list, tag| format!("{tag_list}{tag}")
                            )
                    });

                    let regex: Regex = Regex::new("\\{dev_logs\\}").unwrap();
                    let file_content = regex.replace_all(&file_content, {
                        let template = match dev_log_article_template_rx.recv() {
                            Ok(Ok(UtilityData::String(template))) => template[0].clone(),
                            Err(err) => {
                                error!("{err:?}");
                                return Err(ResponseStatusCode::InternalServerError);
                            }
                            _ => return Err(ResponseStatusCode::InternalServerError),
                        };

                        let id = &proj.0;
                        let id = id.parse::<u32>().unwrap();
                        let dev_log_tree: Graph<NodeIndex, NodeIndex, Directed> = {
                            let mut graph = Graph::new();

                            let dev_log_chain = match dev_log_chain_rx.recv() {
                                Ok(Ok(UtilityData::String(related))) => related,
                                Err(err) => {
                                    error!("{err:?}");
                                    return Err(ResponseStatusCode::InternalServerError);
                                }
                                _ => return Err(ResponseStatusCode::InternalServerError),
                            }
                            .iter()
                            .map(|row| row.split(",").map(|val| String::from(val.trim())))
                            .map(|val| val.map(|val| val.parse::<i32>().unwrap() as NodeIndex))
                            .map(|mut val| (val.next().unwrap(), val.next().unwrap()))
                            .collect::<Vec<(NodeIndex, NodeIndex)>>();

                            graph.extend_with_edges(dev_log_chain);

                            graph
                        };

                        type Name = String;
                        type CreatedDate = Option<String>;
                        type Content = String;

                        type Article = (Name, CreatedDate, Content);

                        let dev_logs: HashMap<i32, Article> = HashMap::from_iter(
                            match dev_log_rx.recv() {
                                Ok(Ok(UtilityData::String(related))) => related,
                                Err(err) => {
                                    error!("{err:?}");
                                    return Err(ResponseStatusCode::InternalServerError);
                                }
                                _ => return Err(ResponseStatusCode::InternalServerError),
                            }
                            .iter()
                            .map(|row| {
                                row.split(",")
                                    .map(|val| String::from(val.trim()))
                                    .collect::<Vec<String>>()
                            })
                            .map(|row| -> (i32, Article) {
                                let created = if &row[2] == NULL {
                                    None
                                } else {
                                    Some(row[2].clone())
                                };

                                return (
                                    row[0].parse::<i32>().unwrap(),
                                    (row[1].clone(), created, row[3].clone()),
                                );
                            }),
                        );

                        trace!("dev log: {dev_logs:#?}");
                        let mut articles: Vec<String> = Vec::new();

                        let start_node =
                            dev_log_tree.neighbors_directed(id.into(), Outgoing).next();

                        if let Some(node) = start_node {
                            let mut node = node;

                            while let Some(next_node) =
                                dev_log_tree.neighbors_directed(node, Outgoing).next()
                            {
                                let id = next_node.index();

                                let (title, time_stamp, content) =
                                    dev_logs.get(&(id as i32)).unwrap();

                                articles.push({
                                    let file_content = template.clone();

                                    let regex: Regex = Regex::new("\\{id\\}").unwrap();
                                    let file_content = regex.replace_all(
                                        &file_content,
                                        format!("{}-{}", title.replace(" ", "_"), id),
                                    );

                                    let regex: Regex = Regex::new("\\{title\\}").unwrap();
                                    let file_content = regex.replace_all(&file_content, title);

                                    let regex: Regex = Regex::new("\\{update\\}").unwrap();
                                    let file_content = regex.replace_all(&file_content, {
                                        match time_stamp {
                                            Some(val) => val,
                                            None => "",
                                        }
                                    });

                                    let regex: Regex = Regex::new("\\{tags\\}").unwrap();
                                    let file_content = regex.replace_all(&file_content, "");

                                    let regex: Regex = Regex::new("\\{content\\}").unwrap();
                                    let file_content = regex.replace_all(&file_content, {
                                        let mut options = Options::empty();
                                        options.insert(Options::ENABLE_STRIKETHROUGH);
                                        let parser = Parser::new_ext(content, options);

                                        let mut html_output = String::new();
                                        html::push_html(&mut html_output, parser);

                                        html_output
                                    });

                                    file_content.to_string()
                                });

                                node = next_node;
                            }
                        }
                        articles
                            .iter()
                            .rev()
                            .fold(String::new(), |acc, val| format!("{}{}", acc, val))
                    });

                    return Ok(
                        Response {
                            status: ResponseStatusCode::Ok,
                            header: HashMap::new(),
                            body: Some(Body {
                                content_type: ContentType::Text(Text::html),
                                content: file_content
                                    .as_bytes()
                                    .to_vec(),
                            })
                        }
                    )
                };
            }
            Err(err) => error!("{err:?}"),
            Ok(_) => {}
        };
    }

    Err(ResponseStatusCode::NotFound)
}

fn calculate_colour(foreground: f64, background: f64, opacity: f64) -> f64 {
    foreground * opacity + (1f64 - opacity) * background
}

ActionBuilder!(
    name = action_boi,
    utility = UtilitySender,
    get = get,
    head = not_allowed_logic,
    post = post,
    put = not_allowed_logic,
    delete = not_allowed_logic,
    connect = not_allowed_logic,
    options = not_allowed_logic,
    trace = not_allowed_logic,
    patch = not_allowed_logic,
    error = default_err_page
);
