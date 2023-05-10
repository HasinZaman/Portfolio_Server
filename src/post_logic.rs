use crate::server::http::body::{Application, ContentType};
use crate::server::http::response::{response::Response, response_status_code::ResponseStatusCode};
use crate::server::http::{body::Body, method::Method, request::parser_error::ParserError};

use crate::sql_reader::DataBase;

use crate::server::setting::ServerSetting;

use std::collections::HashMap;

use log::error;
use serde_json::Value;

pub fn post(
    request: Method,
    _server_settings: &ServerSetting,
    _meta_data: &HashMap<String, String>,
) -> Response {
    if let Method::Post { file, body } = request {
        let post_request: &str = &file;
        return match post_request.trim_matches('/').trim_matches('\\') {
            "get_data" => get_data(body),
            _ => {
                Response {
                    status: ResponseStatusCode::BadRequest,
                    meta_data: HashMap::new(),
                    body: Option::None,
                }
            }
        }
    }
    panic!("post should only used to handle Method::post requests")
}

fn get_data(body: Body) -> Response {
    let body: Value = match parse_json(&body) {
        Ok(val) => val,
        Err(err) => {
            error!("Failed parse\nerror:{:?}", err);
            return Response {
                status: ResponseStatusCode::BadRequest,
                meta_data: HashMap::new(),
                body: Option::None,
            };
        }
    };

    let requested_data: Vec<Value>;
    if body.is_array() {
        requested_data = body.as_array().unwrap().to_vec();
    } else if body.is_string() {
        requested_data = vec![body];
    } else {
        return Response {
            status: ResponseStatusCode::BadRequest,
            meta_data: HashMap::new(),
            body: Option::None,
        };
    }

    let tmp : Vec<String> = requested_data.iter()
        .map(
            |table_name| -> String {
                if !table_name.is_string() {
                    return String::from("")
                }

                let table_name = table_name.as_str().unwrap();

                let rows : Vec<String> = match table_name {
                    "skills" => {
                        DataBase::query(
                            "SELECT * FROM skills",
                            |row| -> String {
                                let id = match row.get::<i32,usize>(0) {
                                    Some(val) => val,
                                    None => return String::from("")
                                };
                                let colour: String = match row.get::<String,usize>(1) {
                                    Some(val) => val,
                                    None => return String::from("")
                                };
                                let symbol: String = match row.get::<String,usize>(2) {
                                    Some(val) => val,
                                    None => return String::from("")
                                };

                                return format!(
                                    "{{\"id\":{}, \"colour\":\"#{}\", \"symbol\":\"{}\"}}",
                                    id,
                                    colour,
                                    symbol
                                );
                            }
                        )
                    },
                    "projects" => {
                        DataBase::query(
                            "SELECT * FROM projects",
                            |row| {
                                let id = match row.get::<i32,usize>(0) {
                                    Some(val) => val,
                                    None => return String::from("")
                                };
                                let _colour = match row.get::<String,usize>(1) {
                                    Some(val) => val,
                                    None => return String::from("")
                                };
                                let name =  match row.get::<String,usize>(2) {
                                    Some(val) => val,
                                    None => return String::from("")
                                };
                                let body =  match row.get::<String,usize>(3) {
                                    Some(val) => val,
                                    None => return String::from("")
                                };
                                let repo =  match row.get::<String,usize>(4) {
                                    Some(val) => val,
                                    None => return String::from("")
                                };
                                let first_push =  match row.get::<String,usize>(5) {
                                    Some(val) => val,
                                    None => return String::from("")
                                };
                                let last_push =  match row.get::<Option<String>,usize>(6) {
                                    Some(val) => {
                                        match val {
                                            Some(val) => val,
                                            None => String::from("null"),
                                        }
                                    },
                                    None => return String::from("")
                                };
                                return format!(
                                    "{{\"Tag\": {id}, \"Start\": \"{first_push}\", \"Update\": \"{last_push}\", \"Title\":\"{name}\", \"Description\": \"{body}\", \"link\":\"{repo}\"}}",
                                );
                            }
                        )
                    },
                    //general values
                    "tag" => {
                        DataBase::query(
                            "SELECT `id`,`colour`,`tag_name`,`tag_type` FROM tag WHERE tag_type!=3",
                            |row:mysql::Row| -> String {
                                let id: i32 = match row.get::<i32,usize>(0) {
                                    Some(val) => val,
                                    None => return String::from("")
                                };

                                let colour: String = match row.get::<String,usize>(1) {
                                    Some(val) => val,
                                    None => return String::from("")
                                };

                                let symbol: String = match row.get::<String,usize>(2) {
                                    Some(val) => val,
                                    None => return String::from("")
                                };

                                let tag_type: i32 = match row.get::<i32,usize>(3) {
                                    Some(val) => val,
                                    None => return String::from("")
                                };

                                return format!(
                                    "{{\"id\":{}, \"colour\":\"#{}\", \"symbol\":\"{}\", \"tag_type\":{}}}",
                                    id,
                                    colour,
                                    symbol,
                                    tag_type
                                );
                            }
                        )
                    },
                    "related" => {
                        DataBase::query(
                            "SELECT `tag_1`,`tag_2` FROM relate_tags",
                            |row:mysql::Row| -> String {
                                let tag_1: i32 = match row.get::<i32,usize>(0) {
                                    Some(val) => val,
                                    None => return String::from("")
                                };

                                let tag_2: String = match row.get::<String,usize>(1) {
                                    Some(val) => val,
                                    None => return String::from("")
                                };

                                return format!(
                                    "{{\"tag_1\":{}, \"tag_2\":{}}}",
                                    tag_1,
                                    tag_2
                                );
                            }
                        )
                    },
                    _ => {
                        vec![String::from("")]
                    },
                };

                return format!(
                    "{{\"name\":\"{}\",\"data\":[{}]}}",
                    table_name,
                    rows.join(",")
                );
            }
        )
        .collect();

    return Response {
        status: ResponseStatusCode::Ok,
        meta_data: HashMap::new(),
        body: Some(Body {
            content_type: ContentType::Application(Application::json),
            content: format!("{:?}", tmp).as_bytes().to_vec(),
        }),
    };
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
