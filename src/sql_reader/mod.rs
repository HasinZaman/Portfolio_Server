use mysql::prelude::*;
use mysql::*;
use serde::de::value::StrDeserializer;
use std::env;
use std::vec::Vec;

#[allow(dead_code)]
#[derive(Debug)]
pub struct DataBase {
    pub db_host: String,
    pub db_port: String,
    pub db_name: String,
    pub db_username: String,
    pub db_password: String,
}

impl DataBase {
    pub fn execute<B, F: Fn(mysql::Row) -> B>(stmp: Statement, row_logic: F) -> Vec<B> {
        let mut conn = get_database().unwrap().get_conn();
        println!("{:?}\n", stmp);
        let results = match conn.exec_iter(&stmp, ()) {
            Ok(ok) => ok,
            Err(err) => {
                panic!("\n{:?} caused {}\n", stmp, err)
            }
        }
        .map(|wrapped_row| row_logic(wrapped_row.unwrap()))
        .collect();
        conn.close(stmp);

        return results;
    }

    pub fn query<B, F: Fn(mysql::Row) -> B>(stmp: &str, row_logic: F) -> Vec<B> {
        let mut conn = get_database().unwrap().get_conn();

        let results = match conn.query_iter(stmp) {
            Ok(ok) => ok,
            Err(err) => {
                panic!("\n{:?} caused {}\n", stmp, err)
            }
        }
        .map(|wrapped_row| row_logic(wrapped_row.unwrap()))
        .collect();

        return results;
    }

    pub fn create_stmp(query: &str) -> Result<mysql::Statement> {
        let mut conn = get_database().unwrap().get_conn();

        conn.prep(query)
    }

    fn get_conn(&self) -> mysql::Conn {
        let url = format!(
            "mysql://{}:{}@{}:{}/{}",
            self.db_username, self.db_password, self.db_host, self.db_port, self.db_name
        );

        let url: Opts = Opts::from_url(&url).unwrap();

        Conn::new(url).unwrap()
    }
}

macro_rules! env_var_to_variable {
    ($key : literal, $var : ident) => {
        match env::var($key) {
            Err(_err) => {
                return None;
            }
            Ok(ok) => $var = ok,
        }
    };
}

fn get_database() -> Option<DataBase> {
    let Ok(db_host) = env::var("DB_host") else {return None};
    let Ok(db_port) = env::var("DB_port") else {return None};
    let Ok(db_name) = env::var("DB_name") else {return None};
    let Ok(db_username) = env::var("DB_username") else {return None};
    let Ok(db_password) = env::var("DB_password") else {return None};

    Some(DataBase {
        db_host: db_host,
        db_port: db_port,
        db_name: db_name,
        db_username: db_username,
        db_password: db_password,
    })
}
