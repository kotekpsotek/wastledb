use sqlparser::{ dialect::PostgreSqlDialect, parser::Parser as SqlParser, ast::{Statement, ObjectName, Ident} };
use format as f;
use Outcomes::*;
use std::{ fs, path::Path, collections::HashMap };

use crate::connection::tcp::{ CommandTypeKeyDiff, SessionData };
use self::additions::unavailable;

#[path ="../additions"]
mod additions {
    pub mod unavailable;
}
/* struct DB_Logs;
impl DB_Logs {
    fn log(message, ) {

    }
} */

#[derive(Debug)]
pub enum Outcomes {
    Error(String), // 1. Reason of error
    Success(Option<String>) // 1. Optional description
}

pub fn process_query(query: &str, auto_connect: Option<crate::connection::tcp::CommandTypeKeyDiff>, session_id: String, sessions: &mut HashMap<String, String>) -> Outcomes {
    let sql_query = query;
    let selected_sql_dialect = PostgreSqlDialect {};
    let parse_operation = SqlParser::parse_sql(&selected_sql_dialect, sql_query);

    match parse_operation {
        Ok(parse_op_result) => {
            // println!("{:?}", parse_op_result);
            // Process SQL Query and do amazing things
            let mut it = 0;
            loop {
                let lexical_sql = parse_op_result[it].clone();
                println!("{:?}", lexical_sql);
                it += 1;

                // Do specific action
                match lexical_sql {
                    // Create SQL database
                    Statement::CreateDatabase { db_name: ObjectName(data_base), if_not_exists: _, location: _, managed_location: _ } => {
                        let db_name_val = &data_base[0].value;
                        
                        if db_name_val.len() > 0 && !unavailable::os_file_system_check_unavailable_characters_into(&db_name_val) {
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
                    /* Statement::CreateTable { 
                        or_replace: _, 
                        temporary: _, 
                        external: _, 
                        global: _, 
                        if_not_exists: _, 
                        name, 
                        columns, 
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

                    }, */
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
