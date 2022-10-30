use sqlparser::{ dialect::PostgreSqlDialect, parser::Parser as SqlParser, ast::{Statement, ObjectName, Ident} };
use format as f;
use Outcomes::*;
use std::{ fs, path::Path };

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

pub fn process_query(query: &str) -> Outcomes {
    let sql_query = query;
    let selected_sql_dialect = PostgreSqlDialect {};
    let parse_operation = SqlParser::parse_sql(&selected_sql_dialect, sql_query);

    match parse_operation {
        Ok(parse_op_result) => {
            println!("{:?}", parse_op_result);
            // Process SQL Query and do amazing things
            let mut it = 0;
            loop {
                let lexical_sql = parse_op_result[it].clone();
                it += 1;

                // Do specific action
                match lexical_sql {
                    // Create SQL database
                    Statement::CreateDatabase { db_name: ObjectName(data_base), if_not_exists: _, location: _, managed_location: _ } => {
                        let db_name_val = &data_base[0].value;
                        
                        // create database + response
                        let loc = f!("../source/dbs/{db_name}", db_name = db_name_val);
                        let db_path = Path::new(loc.as_str());

                            // database can be created only when it actualy doesn't exists
                        if !db_path.exists() {
                            if let Ok(_) = fs::create_dir(db_path) {
                                break Success(None);
                            }; 

                            break Error(f!("Database couldn't been created!"));
                        };
                        
                        break Error(f!("Provided database \"{}\" couldn't be created because this database already exists", db_name_val));
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
