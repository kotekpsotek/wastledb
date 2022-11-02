use sqlparser::{ dialect::AnsiDialect, parser::Parser as SqlParser, ast::{Statement, ObjectName, SetExpr, Expr} };
#[allow(unused)]
use datafusion::prelude::*;
use format as f;
use Outcomes::*;
use std::{ fs, path::Path, collections::HashMap };

use crate::{connection::tcp::{ CommandTypeKeyDiff, SessionData }, management::sql_json::{self, ProcessSQLSupportedQueries}};
use crate::management::sql_json::{ process_sql, ProcessSQLRowField as Field };
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
                // println!("\nQuery:\n\n{:?}", lexical_sql);
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
                                        match process_sql(sql_query, None) {
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
                    Statement::Insert { 
                        or: _, 
                        into, 
                        table_name, 
                        columns: _, // TODO: Add later support for attachement for specific columns
                        overwrite: _,  // TODO: In fetaure add support for INSER OVERWRITE TABLE 'table_name' (now this query works as INSERT INTO).
                        source, 
                        partitioned: _, 
                        after_columns: _, 
                        table: _, // indictaes whethe "table" keyword was attached to INSER OVERWRITE query
                        on: _ 
                    } => {
                        let session_data = serde_json::from_str::<SessionData>(sessions.get(&session_id).unwrap()).unwrap();
                        let user_con_db = session_data.connected_to_database.clone();
                        
                        if into && user_con_db.is_some() { // user must be firsly connected to database
                            // Obtain table name
                            let table_name = &table_name.0[0].value;

                            // Obtain database name to which user is connected
                            let user_con_db = session_data.connected_to_database.unwrap();
                            
                            // Db path
                            let db_path_s = f!("../source/dbs/{}", user_con_db);
                            let db_path = Path::new(&db_path_s);

                            // Db table path
                            let dbt_path_s = f!("{db_loc}/{table}.json", db_loc = db_path_s, table = table_name);
                            let dbt_path = Path::new(&dbt_path_s);

                            // Create only when database and tab;e exists
                            if db_path.exists() && dbt_path.exists() {
                                // Obtain values (to insert for columns) from insert query (whole)
                                let values_from_query = {
                                    // Type which store values for Query must be Values()
                                    if let SetExpr::Values(vals) = *source.body {
                                        let vals = vals.0;
                                        // Ready to insert: List with all rows and it's values to insert
                                        let mut allrows_values_list: Vec<Vec<Field>> = vec![]; // 1st vector = store rows, 2nd vector = store values for columns for single row
                                    
                                        // Iterte over each row with values to insert for each column
                                        for each_row in vals.clone() {
                                            let mut onerow_values_list: Vec<Field> = vec![];

                                            // Extract all values from query and assing it to appropriate type supported by this database or break whole extract operation when some type from query isn't supported by this database
                                            // Iterate over values from one row and extract values (extract in this "scenario" obtain value and it type from query and assign it to datatype supported by this database). When datatype from query isn't supported then whole (insert) operation will be stopped and not performed
                                            for val_ins in &each_row {
                                                if let Expr::Value(dat_type) = val_ins {
                                                    use sqlparser::ast::Value::*; // get types for query (required to assign)
                                                    use sql_json::SupportedSQLDataTypes as sup; // get supported types list
                                                
                                                    // extract types and assign their values to supported datatypes / or stop loop over row values when some type isn't supported
                                                    match dat_type {
                                                        SingleQuotedString(str) | DoubleQuotedString(str) => onerow_values_list.push(Field(str.into(), sup::VARCHAR(None))), // string are interpreted as "TEXT" (up to 16_777_215 characters iin one column) type in this place but also can be VARCHAR (which support to 65_535 characters in one string)
                                                        Null => onerow_values_list.push(Field("null".to_string(), sup::NULL)),
                                                        Boolean(val) => onerow_values_list.push(Field(val.to_string(), sup::BOOLEAN)),
                                                        Number(num, _) => onerow_values_list.push(Field(num.into(), sup::INT)),
                                                        _ => break // for unsuported data types
                                                    }
                                                }
                                                else {
                                                    break;
                                                };
                                            };
                                        
                                            // ACID rules must be fullfiled so: (...to perform query all types must be correctly extracted so (extracted_values_from_row_stored.len() == query_row_values.len()) otheriwise don't perform any slice of whole query to maintain data consistancy and break loop here)
                                            if onerow_values_list.len() == each_row.len() {
                                                allrows_values_list.push(onerow_values_list);
                                            }
                                            else {
                                                break;
                                            };
                                        };
                                    
                                        // ACID principles must be fullfiled so ...rows_query.len() must be equal rows_with_converted_values.len() otherwise operation won't be performed
                                        if allrows_values_list.len() == vals.len() {
                                            allrows_values_list
                                        }
                                        else {
                                            break Error(f!(r#"Some type from your "INSERT" query isn't supported, from this plaintiff whole operation can't be perfomed"#));
                                        }
                                    }
                                    else {
                                        break Error(f!("Query couldn't be executed"));
                                    }
                                };

                                // Create table with new inserted records and save it
                                match process_sql(sql_query, Some(ProcessSQLSupportedQueries::Insert(dbt_path, None, values_from_query))) {
                                    Ok(ready_table) => {
                                        // Put table into string
                                        let table_ready_stri_op = serde_json::to_string(&ready_table);
                                        if let Ok(table_ready_stri) = table_ready_stri_op {
                                            // Save result into table file + return operation result
                                            if let Ok(_) = fs::write(dbt_path, table_ready_stri) {
                                                break Success(Some(f!(r#"INSERT operation has been performed"#)));
                                            }
                                            else {
                                                break Error(f!("Coludn't save results of operation from some reason"));
                                            }
                                        }
                                        else {
                                            break Error(f!("Couldn't convert operation results to JSON format"));
                                        }
                                    },
                                    Err(_) => {println!("Here"); break Error(f!("Values couldn't been inserted to table"))}
                                }
                            }
                            else {
                                break Error(f!("Database to which you're connected doesn't exists | Or table to which you try attach data doesn't exists in database to which you're connected"));
                            }
                        }
                        else {
                            break Error(f!("\"INSERT\" query must include \"INTO\""));
                        }
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
