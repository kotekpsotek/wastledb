use sqlparser::{ dialect::AnsiDialect, parser::Parser as SqlParser, ast::{Statement, ObjectName} };
#[allow(unused)]
use datafusion::prelude::*;
use tokio;
use format as f;
use Outcomes::*;
use std::{ fs, path::Path, collections::HashMap };

use crate::connection::tcp::{ CommandTypeKeyDiff, SessionData };
use crate::management::sql_json::{ process_sql };
use self::additions::unavailable;

#[path ="../additions"]
mod additions {
    pub mod unavailable;
}

#[derive(Debug)]
pub enum Outcomes {
    Error(String), // 1. Reason of error
    Success(Option<String>) // 1. Optional description
}

pub fn process_query(query: &str, auto_connect: Option<crate::connection::tcp::CommandTypeKeyDiff>, session_id: String, sessions: &mut HashMap<String, String>) -> Outcomes {
    let sql_query = query;
    let selected_sql_dialect = AnsiDialect {};
    let parse_operation = SqlParser::parse_sql(&selected_sql_dialect, sql_query);

    match parse_operation {
        Ok(parse_op_result) => {
            // println!("{:?}", parse_op_result);
            // Process SQL Query and do amazing things
            let mut it = 0;
            loop {
                let lexical_sql = parse_op_result[it].clone();
                // println!("{:?}", lexical_sql);
                it += 1;

                // Do specific action
                match lexical_sql {
                    // Create SQL database
                    Statement::CreateDatabase { db_name: ObjectName(data_base), if_not_exists: _, location: _, managed_location: _ } => {
                        let db_name_val = &data_base[0].value;
                        
                        if db_name_val.len() > 0 && !unavailable::os_file_system_check_unavailable_characters_into(&db_name_val) && !unavailable::FILENAMES_WINDOWS.contains(&db_name_val.as_str()) {
                            let session_data = sessions.get(&session_id).unwrap(); // here session must exists FIXME: In feature (after addition system to remove session after crossed "session persists time (TTL otherwise)" time that session can stop exists here)
                            let mut session_data = serde_json::from_str::<SessionData>(session_data).unwrap();
                            
                            // create database + response
                            let loc = f!("../source/dbs/{db_name}", db_name = db_name_val);
                            let db_path = Path::new(loc.as_str());
    
                                // database can be created only when it actualy doesn't exists
                            if !db_path.exists() {
                                if let Ok(_) = fs::create_dir(db_path) {
                                        // Connect user with database when he would like get that by place appropriate command
                                    if let Some(CommandTypeKeyDiff { name: _, value }) = auto_connect {
                                        if value == "true" {
                                            // Update session on session storage
                                            session_data.connected_to_database = Some(db_name_val.to_owned());
                                            let session_data = serde_json::to_string(&session_data).unwrap();
                                            sessions.insert(session_id, session_data);
                                        };
                                    };
    
                                    // Send result
                                    break Success(None);
                                }; 
    
                                break Error(f!("Database couldn't been created!"));
                            };
                            
                            break Error(f!("Provided database \"{}\" couldn't be created because this database already exists", db_name_val));
                        }
                        else {
                            break Error(f!("Database name is not correct!"));
                        }
                    },
                    Statement::CreateTable { 
                        or_replace: _, 
                        temporary: _, 
                        external: _, 
                        global: _, 
                        if_not_exists: _, 
                        name, 
                        columns: _, 
                        constraints: _, 
                        hive_distribution: _, 
                        hive_formats: _, 
                        table_properties: _, 
                        with_options: _, 
                        file_format: _, 
                        location: _, 
                        query: _on1, 
                        without_rowid: _, 
                        like: _, 
                        clone: _, 
                        engine: _, 
                        default_charset: _, 
                        collation: _, 
                        on_commit: _, 
                        on_cluster: _ 
                    } => {
                        let session_data = serde_json::from_str::<SessionData>(sessions.get(&session_id).unwrap()).unwrap();
                        
                        if session_data.connected_to_database.is_some() {
                            if Path::new(&f!("../source/dbs/{db}", db = session_data.connected_to_database.clone().unwrap())).exists() {
                                let table_name = if name.0.len() > 0 {
                                    Some(&name.0[0].value)
                                }
                                else {
                                    None
                                };

                                if let Some(table_name) = table_name {
                                    let connection_db = session_data.connected_to_database.clone();
                                    let f_p_s = f!("../source/dbs/{db}/{tb}.json", db = connection_db.unwrap(), tb = table_name);
                                    let f_p = Path::new(&f_p_s);

                                    if !f_p.exists() {                                    
                                        // + execute query by apache arrow-datafusion on created path
                                        match process_sql(sql_query) {
                                            Ok(table) => {
                                                let r_json = serde_json::to_string(&table); // for pretty format data use serde_json::to_string_pretty(&table), but it will use unnecessary characters (for pretty print u can use nested VS Code .json formater) 

                                                if let Err(_) = r_json {
                                                    break Error(f!("Couldn't create table"));
                                                };

                                                if let Ok(_) = fs::write(f_p, r_json.unwrap()) {
                                                    break Success(None);
                                                }
                                                else {
                                                    break Error(f!("Couldn't create table"));
                                                }
                                            },
                                            // is returned for exmaple when: to column is attached unsupported type by function compared "process_sql" function
                                            Err(_) => break Error(f!("Couldn't create table"))
                                        }
                                    }
                                    else {
                                        break Error(f!("This table already exists so it can't be re-created"));
                                    }
                                }
                            };

                            break Error(f!("Database to which you're connected doesn't exists!"));
                        }

                        break Error(f!("You're not connected to any database. In order to execute this command you must be connected!"));
                    },
                    _ => {
                        if parse_op_result.len() > it {
                            continue;
                        }
                        else {
                            break Error(f!("SQL query couldn't been performed"));
                        }
                    }
                }
            }
        },
        Err(_) => return Error(f!("SQL Syntax Error"))
    }
}

#[test]
fn sql_parser_test() {
    use sqlparser;
    
    // sql query to parse
    let sql = "CREATE DATABASE kotki"; // when SQL syntax is incorrect then ParrserError is returned
    
    // parse sql
    let sql_dialect = sqlparser::dialect::PostgreSqlDialect {};
    let parsed_sql = sqlparser::parser::Parser::parse_sql(&sql_dialect, sql).unwrap();
    println!("{:?}", parsed_sql)
}

#[tokio::test]
async fn create_table() {
    let cx = SessionContext::new();
    cx.register_json("ready", "../source/dbs/test/table.json", NdJsonReadOptions::default()).await.expect("Couldn't register table.json file");

    let sql_t = cx.sql("INSERT INTO new_table (col1, col2) VALUES (1, 2)").await.unwrap();

    let _dat = sql_t.collect().await.unwrap();
}
