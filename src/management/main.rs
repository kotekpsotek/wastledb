use sqlparser::{ dialect::AnsiDialect, parser::Parser as SqlParser, ast::{Statement, ObjectName, SetExpr, Expr, DataType, ColumnOptionDef, ObjectType, SelectItem, TableFactor} };
#[allow(unused)]
use datafusion::prelude::*;
use format as f;
use Outcomes::*;
use std::{ fs, path::Path, collections::HashMap };

use crate::connection::tcp::{ CommandTypeKeyDiff, SessionData };
use crate::management::sql_json::{ self, process_sql, ProcessSQLRowField as Field, SupportedSQLDataTypes, SupportedSQLColumnConstraints, ProcessSQLSupportedQueries, InsertOperations, ConvertSQLParserTypesToSupported, ConvertSQLParserOptionsToSupportedConstraints };
use self::additions::unavailable;

#[path ="../additions"]
mod additions {
    pub mod unavailable;
}

impl ConvertSQLParserTypesToSupported for DataType {}
impl ConvertSQLParserOptionsToSupportedConstraints for ColumnOptionDef {}

#[derive(Debug)]
pub enum Outcomes {
    Error(String), // 1. Reason of error
    Success(Option<String>) // 1. Optional description
}

/// Obtain information whether user is connected to database and database name when is
fn get_database_user_connected_to(sessions: &mut HashMap<String, String>, session_id: &String) -> Option<String> {
    let session_data = serde_json::from_str::<SessionData>(sessions.get(session_id).unwrap()).unwrap();
    let user_con_db = session_data.connected_to_database.clone();

    return user_con_db;
}

/// Get path (struct PathBuf) to table located into database
fn get_dbtable_path(db_name: &String, table_name: &String) -> std::path::PathBuf {
    let table_path_str = f!("../source/dbs/{0}/{1}.json", db_name, table_name);
    let table_path = Path::new(&table_path_str).to_owned();
    return table_path;
}

/// Process sended sql query
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
                        let session_data = get_database_user_connected_to(sessions, &session_id);

                        if session_data.is_some() {
                            let database_name = session_data.unwrap();
                            if Path::new(&f!("../source/dbs/{db}", db = database_name)).exists() {
                                // Obtain table name and put it into Option<String>
                                let table_name = if name.0.len() > 0 {
                                    Some(&name.0[0].value)
                                }
                                else {
                                    None
                                };

                                // Table name must be attached in query!
                                if let Some(table_name) = table_name {
                                    let f_p = get_dbtable_path(&database_name, table_name);

                                    if !f_p.exists() {                                    
                                        // obtain column properties in order to allow create a table
                                        let mut columns_cv = vec![] as Vec<(String, SupportedSQLDataTypes, Option<Vec<SupportedSQLColumnConstraints>>)>;
                                        for column in &columns {
                                            // obtain required properties from column
                                            let col_name = column.name.clone().value;
                                            let col_data_type = if let Some(r#type) = DataType::convert(&column.data_type) {
                                                r#type
                                            }
                                            else {
                                                // when type from query isn't supported then break whole loop from ACID model reason
                                                break;
                                            };
                                            let col_constraints = {
                                                let mut constraints = vec![] as Vec<SupportedSQLColumnConstraints>;
                                                if column.options.len() > 0 {
                                                    for option in column.options.clone() {
                                                        if let Some(constraint) = ColumnOptionDef::convert(option) {
                                                            constraints.push(constraint)
                                                        }
                                                        else {
                                                            // When option isn't supported
                                                            break;
                                                        }
                                                    }
                                                }
                                                constraints
                                            };

                                            // compose column and attach it to vector
                                            let ready_column = (col_name, col_data_type, {
                                                if col_constraints.len() > 0 {
                                                    Some(col_constraints)
                                                }
                                                else {
                                                    None
                                                }
                                            });
                                            columns_cv.push(ready_column);
                                        };
                                        if columns_cv.len() != columns.len() { // when all columns wasn't correctly processed
                                            break Error(f!("In query you attach unsupported type or this has been caused by other query inconsistent factor"));
                                        };

                                        // Create table in json format and write it to file located into database folder. Table file name is table name attached to query
                                        match process_sql(ProcessSQLSupportedQueries::CreateTable(
                                            table_name.into(), 
                                            columns_cv
                                        )) {
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
                        columns, // TODO: Add later support for attachement for specific columns
                        overwrite,
                        source, 
                        partitioned: _, 
                        after_columns: _, 
                        table: _, // indictaes whethe "table" keyword was attached to INSER OVERWRITE query
                        on: _ 
                    } => {
                        let session_data = serde_json::from_str::<SessionData>(sessions.get(&session_id).unwrap()).unwrap();
                        let user_con_db = session_data.connected_to_database.clone();
                        
                        if user_con_db.is_some() { // user must be firsly connected to database
                            // Support for both operations types "INSERT INTO" and "INSERT OVERWRITE TABLE"
                            let op_type: Option<InsertOperations> = {
                                if into {
                                    Some(InsertOperations::Into)
                                }
                                else if overwrite {
                                    Some(InsertOperations::Overwrite)
                                }
                                else {
                                    None
                                }
                            };
                            
                                // ...rust required safeguards for support only 2 insert operations
                            if op_type.is_some() {
                                let op_type = op_type.unwrap();

                                // Obtain table name
                                let table_name = &table_name.0[0].value;

                                // Obtain database name to which user is connected
                                let user_con_db = session_data.connected_to_database.unwrap();

                                // Db table path
                                let dbt_path = get_dbtable_path(&user_con_db, table_name);

                                // Create only when database and tab;e exists
                                if dbt_path.exists() {
                                    // Obtain for which coulmns operation must be performed only
                                    let columns_from_query = {
                                        // To return Some(_) columns len from lexer must be greater then 0 hence them must exists
                                        if columns.len() > 0 {
                                            let mut c_r = Vec::new() as Vec<String>;
                                            for column in columns {
                                                let column_name = column.value;
                                                c_r.push(column_name);
                                            };
                                            Some(c_r)
                                        }
                                        else {
                                            None
                                        }
                                    };
                                    
                                    // Obtain values (to insert for columns) from insert query (whole) // Error: When value coudn't be converted or vector with converted results is shorter then this from query values then loop is break inside brackets "{}" and further (below) code won't be performing as next
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
                                    // When operation must be performed for specific columns then columns correcteness and whether that operation can be performed is check inside process_sql function -> because there exists deserialized JSON table
                                    match process_sql(ProcessSQLSupportedQueries::Insert(&dbt_path, columns_from_query, values_from_query, op_type)) {
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
                                        Err(_) => {break Error(f!("Values couldn't been inserted to table"))}
                                    }
                                }
                                else {
                                    break Error(f!("Database to which you're connected doesn't exists | Or table to which you try attach data doesn't exists in database to which you're connected"));
                                }
                            }
                            else {
                                break Error(f!("This \"INSERT\" operation isn't supported"));
                            };
                        }
                        else {
                            break Error(f!("\"INSERT\" query must include \"INTO\""));
                        }
                    },
                    Statement::Truncate { 
                        table_name, 
                        partitions: _ 
                    } => {
                        // Get whether user is connected to database and database name to which is
                        let user_con_db = get_database_user_connected_to(sessions, &session_id);

                        if user_con_db.is_some() {
                            let user_con_db = user_con_db.unwrap();
                            
                            // Truncate table rows operation
                            let table_name = &table_name.0[0].value;
                            let table_path = get_dbtable_path(&user_con_db, table_name);

                            // Perform operation only when table exists into specified database
                            if table_path.exists() {
                                // Begin truncate operation and its results
                                match process_sql(ProcessSQLSupportedQueries::Truncate(&table_path)) {
                                    Ok(tr_table) => {
                                        // serialize table to String
                                        let ready_table = {
                                            if let Ok(table_str) = serde_json::to_string(&tr_table) {
                                                table_str
                                            }
                                            else {
                                                break Error(f!("Coludn't truncate table"));
                                            }
                                        };

                                        // Save truncated table to file
                                        match fs::write(table_path, ready_table) {
                                            Ok(_) => break Success(None),
                                            Err(_) => break Error(f!("Durning operation table begin stop existing"))
                                        }
                                    },
                                    Err(_) => break Error(f!("Coludn't truncate table"))
                                }
                            }
                            else {
                                break Error(f!("Entered table doesn't exist within Database"));
                            }
                        }
                    },
                    Statement::Drop { // For both table and database but indicator on what unit operation should be performed is "object_type" property
                        object_type, 
                        if_exists:_, 
                        names, 
                        cascade: _, 
                        restrict:_, 
                        purge: _ 
                    } => {
                        let object_name = &(&(&names[0] as &ObjectName).0[0] as &sqlparser::ast::Ident).value;
                        
                        match object_type {
                            ObjectType::Table => {
                                let connected_to_database = get_database_user_connected_to(sessions, &session_id);

                                if let Some(database) = connected_to_database {
                                    let table_path = get_dbtable_path(&database, &object_name);
                                    
                                    if table_path.exists() {
                                        match fs::remove_file(table_path) {
                                            Ok(_) => break Success(None),
                                            Err(_) => break Error(f!("Couldn't delete table"))
                                        }
                                    }
                                    else {
                                        break Error(f!("This table doesn't exists"));
                                    }
                                }
                                else {
                                    break Error(f!("To perform this operation you must be connected to database firstly!"));
                                }
                            },
                            _ => break Error(f!("Couldn't perform operation"))
                        }
                    },
                    Statement::Query(query) => {
                        match *query.body {
                            SetExpr::Select(select_query) => {
                                if let Some(db) = get_database_user_connected_to(sessions, &session_id) {
                                        // Extract data from parser SQL query
                                    let sel_proj = { // SELECT filter ... -> (// Always Must be returned list containing string with speecific name or single "all" value // When error in identifing selection then return Error as operation result)
                                        let sel_proj = select_query.projection;
                                        if sel_proj.len() > 0 {
                                            let mut result_proj = vec![] as Vec<String>;
                                            for proj in &sel_proj {
                                                match proj {
                                                    SelectItem::Wildcard => { // select all result fields from record
                                                        result_proj.push(String::from("all"));
                                                        break;
                                                    },
                                                    SelectItem::UnnamedExpr(inside) => { // single attribute to display
                                                        if let Expr::Identifier(indent) = inside.clone() {
                                                            if indent.value.len() > 0 {
                                                                result_proj.push(indent.value)
                                                            }
                                                            else {
                                                                // Don't allow to empty field names
                                                                break;
                                                            }
                                                        }
                                                        else {
                                                            break;
                                                        }
                                                    },
                                                    _ => break
                                                }
                                            };

                                            if result_proj.len() == sel_proj.len() {
                                                result_proj
                                            }
                                            else {
                                                break Error(f!("Incompatible projection values!"));
                                            }
                                        }
                                        else {
                                            break Error(f!("Incompatible projection values!"));
                                        }
                                    };
                                    let sel_from_table = { // ... FROM "table_name" (// Benath is always not empty string as this representing table name, when something went wrong durning check then opeartion is break with returned "Error(reason_string)" outside)
                                    let from = select_query.from;
                                        // obtain single "table name"
                                    if from.len() == 1 {
                                        let from = &from[0].relation;
                                        if let TableFactor::Table { name, alias: _, args: _, with_hints: _ } = from {
                                                // Go ahead only when list with parsed table_names isn't empty
                                            if name.0.len() > 0 {
                                                let table_name = name.0[0].value.to_owned();
                                                    // table name length must not be empty e.g: empty quotes "" or ''
                                                if table_name.len() > 0 {
                                                    table_name
                                                }
                                                else {
                                                    break Error(f!("Table name doesn't fullfill requirements"));
                                                }
                                            }
                                            else {
                                                break Error(f!("Couldn't obtain table name"));
                                            }
                                        }
                                        else {
                                            break Error(f!("Couldn't obtain table name"));
                                        }
                                    }
                                    else {
                                        // Don't allow to add multiple tables to perf selection from
                                        break Error(f!("Incompatible selection from format!"));
                                    }
                                    };
                                    let sel_statements = select_query.selection; // WHERE ...

                                    let table_path = get_dbtable_path(&db, &sel_from_table);
                                    if table_path.exists() {
                                        match process_sql(ProcessSQLSupportedQueries::Select(&table_path, sel_proj, sel_statements)) {
                                            Ok(table_records) => {
                                                    // Convert obtained record to JSON format and when serialization has been finalized with error return communicate otherwise obtained data in JSON format
                                                let records_str = serde_json::to_string(&table_records.rows)
                                                    .map_or_else(|_err| (false, String::new()), |suc| (true, suc));
                                                
                                                if records_str.0 {
                                                    // Send to user only finded rows without table boilerplate
                                                    // When rows are empty then send "null" as records result
                                                    break Success(Some(records_str.1));
                                                }
                                                else {
                                                    break Error(f!("Couldn't convert records to redable form"));
                                                }
                                            },
                                            Err(_) => ()
                                        }
                                    }
                                    else {
                                        break Error(f!("Table given by you doesn't exists in database to which you're connected"));
                                    }
                                }
                                else {
                                    break Error(f!("You're not connected to database"));
                                };

                                // println!("\nExpr: {:?}\n\nTable name: {}", sel_proj, sel_from);
                                break Success(None);
                            },
                            _ => break Error(f!("Not supported query"))
                        };
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
