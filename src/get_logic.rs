use std::{collections::HashMap, mem, io::Write};
use flate2::{write::{GzEncoder, DeflateEncoder, ZlibDecoder, ZlibEncoder}};
use hex_rgb::Color;

use log::trace;
use petgraph::{adj::NodeIndex, Graph, Directed, Direction::Outgoing};
use regex::Regex;
use pulldown_cmark::{Parser, Options, html};

use crate::{server::{http::{response::{response::Response, response_status_code::ResponseStatusCode}, body::{ContentType, Body, Image, Text, Video, Application}, method::Method}, file_reader::{self, Error}, setting::ServerSetting}, sql_reader::DataBase};

pub fn get(request: Method, server_settings: &ServerSetting, meta_data: &HashMap<String, String>,) -> Response {
    if let Method::Get{ file } = request {
        let host: &str = match meta_data.get("host") {
            Some(host) => host,
            None => {
                return Response {
                    status: ResponseStatusCode::ImATeapot,
                    meta_data: HashMap::new(),
                    body: None,
                }
            }
        };

        let (host_path, allowed_extension) = match server_settings.paths.get(host) {
            Some(host_path) => (&host_path.path, &host_path.allow),
            None => {
                return Response {
                    status: ResponseStatusCode::NotFound,
                    meta_data: HashMap::new(),
                    body: None,
                }
            }
        };

        trace!("file:{}", file);

        if let Some(val) = is_project_name(&file[1..]) {
            trace!("{:?}", val);
            let dev_log_template = get_template_path_buf(String::from("/src/template/dev_log.html.template"), host_path).unwrap();

            let file_content = file_reader::get_file_content_string(dev_log_template.as_path());
            let file_content = file_content.unwrap();

            let regex : Regex = Regex::new("\\{project_name\\}").unwrap();
            let file_content = regex.replace_all(&file_content, val.1);

            let regex : Regex = Regex::new("\\{summary\\}").unwrap();
            let file_content = regex.replace_all(&file_content, val.2);

            let regex : Regex = Regex::new("\\{link\\}").unwrap();
            let file_content = regex.replace_all(&file_content, val.3);
            
            let regex : Regex = Regex::new("\\{start\\}").unwrap();
            let file_content = regex.replace_all(&file_content, val.4);
            let regex : Regex = Regex::new("\\{update\\}").unwrap();
            let file_content = regex.replace_all(&file_content, val.5);

            let regex: Regex = Regex::new("\\{tags\\}").unwrap();
            let file_content = regex.replace_all(&file_content, get_related_skill_tags(&val.0));
            
            let regex: Regex = Regex::new("\\{dev_logs\\}").unwrap();
            let tmp = val.0;
            let tmp: u32 = tmp.try_into().unwrap();
            let file_content = regex.replace_all(&file_content, project_dev_logs(tmp, &host_path));

            
            //add contents of file

            return body_compression(
                &meta_data,
                HashMap::from([(
                    "Cache-Control".to_string(),
                    "private".to_string(),
                )]),
                Body {
                    content_type: ContentType::Text(Text::html),
                    content: file_content
                        .as_bytes()
                        .to_vec(),
                }
            );
        }
        else {
            let path_buf = match get_path_buf(file, host_path, allowed_extension) {
                Ok(value) => value,
                Err(value) => return value,
            };
            
            //check if it's a template
                //generate file
            //
    
            let file_name = match path_buf.file_name() {
                Some(name) => name.to_str().unwrap(),
                None => panic!(
                    "FileReader::parse does not return file name. {:?}",
                    path_buf
                ),
            };
    
            let body = file_reader::get_file_content_bytes(&path_buf).unwrap();
    
            match file_name {
                "404.html" => {
                    return not_found_response()
                },
                _ => {
                    let content_type = path_buf.extension().unwrap();
                    let content_type = content_type.to_str().unwrap();
    
                    let content_type: ContentType = match content_type {
                        "gif" => ContentType::Image(Image::gif),
                        "jpg" | "jpeg" | "jpe" | "jfif" => ContentType::Image(Image::jpeg),
                        "png" => ContentType::Image(Image::png),
                        "tif" | "tiff" => ContentType::Image(Image::tiff),
                        "css" => ContentType::Text(Text::css),
                        "csv" => ContentType::Text(Text::csv),
                        "html" => ContentType::Text(Text::html),
                        "js" => ContentType::Text(Text::javascript),
                        "xml" => ContentType::Text(Text::xml),
                        "mpeg" => ContentType::Video(Video::mpeg),
                        "mp4" => ContentType::Video(Video::mp4),
                        "webm" => ContentType::Video(Video::webm),
                        "svg" => ContentType::Image(Image::svg_xml),
                        "ico" => ContentType::Image(Image::x_icon),
                        "woff" => ContentType::Application(Application::woff),
                        _ => {
                            return Response {
                                status: ResponseStatusCode::UnsupportedMediaType,
                                meta_data: HashMap::new(),
                                body: None,
                            }
                        }
                    };
    
                    return body_compression(
                        &meta_data,
                        HashMap::from([(
                            "Cache-Control".to_string(),
                            "private".to_string(),
                        )]),
                        Body {
                            content_type: content_type,
                            content: body,
                        }
                    )
                }
            }
        }

        
    }
    panic!("should only get get");
}

fn body_compression(request_meta_data: &HashMap<String, String>, mut response_meta_data: HashMap<String, String>, mut body: Body) -> Response {
    if !request_meta_data.contains_key("accept-encoding") || true {
        return Response {
            status: ResponseStatusCode::Ok,
            meta_data: response_meta_data,
            body: Some(body),
        }
    }
    let accepted = request_meta_data.get("accept-encoding").unwrap()
        .to_string()
        .replace(' ', "");

    trace!("accepted encoder:{:?}", accepted);

    for decoder in accepted.split(',') {
        use flate2::Compression;

        match decoder {
            "gzip" => {

                let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
                encoder.write(&body.content);

                body.content = encoder.finish().unwrap();
                response_meta_data.insert(String::from("Content-Encoding"), String::from("gzip"));
                break;
            },
            "deflate" => {
                let mut encoder = DeflateEncoder::new(Vec::new(), Compression::default());
                encoder.write(&body.content);

                body.content = encoder.finish().unwrap();
                response_meta_data.insert(String::from("Content-Encoding"), String::from("deflate"));
                break;
            },
            "zlib" => {
                let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
                encoder.write(&body.content);

                body.content = encoder.finish().unwrap();
                response_meta_data.insert(String::from("Content-Encoding"), String::from("zlib"));
                break;
            },
            _ => {
                
            }
        }
    }

    Response {
        status: ResponseStatusCode::Ok,
        meta_data: response_meta_data,
        body: Some(body),
    }
}



fn is_project_name(file: &str) -> Option<(i32, String, String, String, String, String)> {
    let mut vals: Vec<(i32, String, String, String, String, String)> = DataBase::query(
        &format!("SELECT * FROM projects WHERE tag_name = '{}'", &file.replace("_", " ")),
        |row| {
            let id = row.get::<i32,usize>(0).unwrap();
            let _colour = row.get::<String,usize>(1).unwrap();
            let name =  row.get::<String,usize>(2).unwrap();
            let body =  row.get::<String,usize>(3).unwrap();
            let repo =  row.get::<String,usize>(4).unwrap();
            let first_push =  row.get::<String,usize>(5).unwrap();
            let last_push = match row.get::<Option<String>,usize>(6).unwrap() {
                Some(val) => val,
                None => String::from("Present"),
            };

            return (id, name, body, repo, first_push, last_push);
        }
    );

    if vals.len() == 0 {
        return None;
    }

    Some(mem::take(&mut vals[0]))
}

fn project_dev_logs(id: u32, host_path: &str) -> String{
    let mut dev_log_tree: Graph<NodeIndex, NodeIndex, Directed> = Graph::new();

    let mut edges: Vec<(NodeIndex, NodeIndex)> = DataBase::query(
        "SELECT Distinct tag_1, tag_2 FROM relate_tags, tag WHERE (tag_1 = id AND tag_type = 3) OR (tag_2 = id AND tag_type = 3)",
        |row| {
            let tag_1: NodeIndex = row.get::<NodeIndex,usize>(0).unwrap();
            let tag_2: NodeIndex = row.get::<NodeIndex,usize>(1).unwrap();

            (tag_1, tag_2)
        }
    );
    
    dev_log_tree.extend_with_edges(edges);

    let mut dev_logs = HashMap::new();

    let tmp = DataBase::query(
        "SELECT id, tag_name, created, body FROM dev_log, tag WHERE id=tag_id",
        |row| {
            let id: NodeIndex = row.get::<NodeIndex, usize>(0).unwrap();
            let name = row.get::<String, usize>(1).unwrap();
            let time_stamp = row.get::<Option<String>,usize>(2).unwrap();
            let body = row.get::<String,usize>(3).unwrap();

            (id, name, time_stamp, body)
        }
    );
    tmp.iter()
        .for_each(
            |(id, name, time_stamp, body)| {
                dev_logs.insert(id, (name, time_stamp, body));
            }
        );

    let mut articles: Vec<String> = Vec::new();

    let start_node = dev_log_tree.neighbors_directed(id.into(), Outgoing).next();
    if let None = start_node {
        return String::from("");
    }
    
    let mut node = start_node.unwrap();
    while let Some(next_node) = dev_log_tree.neighbors_directed(node, Outgoing).next() {
        let tmp = next_node.index();

        let dev_log = dev_logs.get(&(tmp as u32)).unwrap();
        articles.push(get_article(&(next_node.index() as i32), dev_log.0, dev_log.1, dev_log.2, host_path));
        node = next_node;
    }

    articles.iter()
        .rev()
        .fold(String::new(), |acc, val| format!("{}{}", acc, val))
}

fn get_article(id: &i32, title: &str, time_stamp: &Option<String>, body: &str, host_path: &str) -> String {
    let dev_log_template = get_template_path_buf(String::from("/src/template/dev_log.article.html.template"), host_path).unwrap();

    let file_content = file_reader::get_file_content_string(dev_log_template.as_path());
    let file_content = file_content.unwrap();

    let regex: Regex = Regex::new("\\{id\\}").unwrap();
    let file_content = regex.replace_all(
        &file_content,
        format!(
            "{}-{}",
            title.replace(" ", "_"),
            id
        )
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
    let file_content = regex.replace_all(&file_content, get_related_skill_tags(id));

    let regex: Regex = Regex::new("\\{content\\}").unwrap();
    let file_content = regex.replace_all(
        &file_content, 
        {
            let mut options = Options::empty();
            options.insert(Options::ENABLE_STRIKETHROUGH);
            let parser = Parser::new_ext(body, options);

            let mut html_output = String::new();
            html::push_html(&mut html_output, parser);

            html_output
        }
    );

    file_content.to_string()
}

fn get_path_buf(file: String, host_path: &str, allowed_extension: &Vec<String>) -> Result<std::path::PathBuf, Response> {
    let parse = file_reader::parse(
        &file,
        &host_path,
        move |extension: &str| allowed_extension.iter().any(|ext| extension == ext)
    );

    match &*file {
        "dev_log_prototype.html" => {
            Err(
                Response {
                    status: ResponseStatusCode::NotFound,
                    meta_data: HashMap::new(),
                    body: Some(Body {
                        content_type: ContentType::Text(Text::html),
                        content: format!(
                            "<html><h1>Error code: {}</h1><p>{}</p></html>",
                            ResponseStatusCode::NotFound.get_code(),
                            ResponseStatusCode::NotFound.to_string()
                        )
                        .as_bytes()
                        .to_vec(),
                    }),
                }
            )
        },
        _=> {
            Ok(
                match parse {
                    Ok(path_buf) => {
                        path_buf
                    },
                    Err(Error::FileDoesNotExist) => {
                        return Err(not_found_response())
                    }
                    _ => {
                        return Err(
                            Response {
                                status: ResponseStatusCode::NotFound,
                                meta_data: HashMap::new(),
                                body: Some(Body {
                                    content_type: ContentType::Text(Text::html),
                                    content: format!(
                                        "<html><h1>Error code: {}</h1><p>{}</p></html>",
                                        ResponseStatusCode::NotAcceptable.get_code(),
                                        ResponseStatusCode::NotAcceptable.to_string()
                                    )
                                    .as_bytes()
                                    .to_vec(),
                                }),
                            }
                        );
                    }
                }
            )
        }
    }
}

fn get_template_path_buf(file: String, host_path: &str) -> Result<std::path::PathBuf, Response> {
    get_path_buf(file, host_path, &vec![String::from("template")])
}

fn get_related_skill_tags(id: &i32) -> String{
    let tags = DataBase::query(
        &format!("SELECT * FROM tag WHERE EXISTS(SELECT * FROM relate_tags WHERE relate_tags.tag_1 = {} AND relate_tags.tag_2 = tag.id) AND tag_type != 3", id),
        |row| {
            let id: i32 = row.get::<i32,usize>(0).unwrap();

            let colour: String = row.get::<String,usize>(1).unwrap();

            let name: String = row.get::<String,usize>(3).unwrap();

            //let _tag_type: i32 = row.get::<i32,usize>(3).unwrap();

            let bg_colour = Color::new(&format!("#{}", colour)).unwrap();

            let calculate = |foreground : f64, background : f64, opacity: f64| foreground * opacity + (1f64 - opacity) * background;
            
            let (r,g,b) = (
                calculate(bg_colour.red as f64, 0f64, 0.75f64),
                calculate(bg_colour.green as f64, 0f64, 0.75f64),
                calculate(bg_colour.blue as f64, 0f64, 0.75f64),
            );

            format!("<div id=\"{id}\" class=\"tag\" style=\"border-color:#{colour};background-color:rgb({r},{g},{b});\">{name}</div>")
        }
    );

    tags.join("")
}

fn not_found_response() -> Response {
    Response {
        status: ResponseStatusCode::NotFound,
        meta_data: HashMap::new(),
        body: Some(Body {
            content_type: ContentType::Text(Text::html),
            content: format!(
                "<html><h1>Error code: {}</h1><p>{}</p></html>",
                ResponseStatusCode::NotFound.get_code(),
                ResponseStatusCode::NotFound.to_string()
            )
            .as_bytes()
            .to_vec(),
        }),
    }
}