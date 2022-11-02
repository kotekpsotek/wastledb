#![allow(unused)]
use std::{ path::Path, fs };

use serde::{ self, Serialize, Deserialize };
use sqlparser::{ self, ast::{ Statement, DataType, ColumnOptionDef, ColumnOption } };
use Statement::*;

/* Create table in json format */
#[derive(Serialize, Deserialize, Debug, Clone)]
/// Represent SQL table created from query in JSON format
pub struct JsonSQLTable {
    /// table name
    pub name: String,
    /// columns schema
    pub columns: Vec<JsonSQLTableColumn>,
    /// rows with columns. This value can be represented by None in moment when: table is now created or it doesn't have got any records inside
    pub rows: Option<Vec<Vec<JsonSQLTableColumnRow>>>
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
/// Represent all supported SQL collumn types in this database
pub enum SupportedSQLDataTypes {
    INT,
    FLOAT,
    TEXT,
    VARCHAR(Option<u16>), // can store maximum 65_535 bytes
    LONGTEXT,
    DATE,
    DATETIMESTAMP,
    NULL,
    BOOLEAN
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[allow(non_camel_case_types)]
/// Represent all supported column constraints in this database 
pub enum SupportedSQLColumnConstraints {
    PRIMARY_KEY,
    FOREGIN_KEY,
    DEFAULT(String),
    NOT_NULL,
    UNIQUE
}

#[derive(Serialize, Deserialize, Debug, Clone)]
/// Represent each column placed in JsonSQLTable
pub struct JsonSQLTableColumn {
    /// column name
    pub name: String,
    /// column data type
    pub d_type: SupportedSQLDataTypes,
    /// optional column constraints
    pub constraints: Option<Vec<SupportedSQLColumnConstraints>>
}

#[derive(Serialize, Deserialize, Debug, Clone)]
/// Represent each row with data for "JsonSQLTable" struct
pub struct JsonSQLTableColumnRow {
    pub col: String,
    pub value: String 
}

#[derive(Debug, PartialEq)]
pub struct ProcessSQLRowField(pub String, pub SupportedSQLDataTypes); // 1. field value, 2. Field data type (only supported datatypes)

type TableName = String;
type TablePath<'s> = &'s Path;
type ColumnName = String;
type ActionOnlyForTheseColumns = Vec<String>;
type RowsToProcess = Vec<ProcessSQLRowField>;

#[derive(Debug)]
pub enum ProcessSQLSupportedQueries<'x> {
    Insert(TablePath<'x>, Option<ActionOnlyForTheseColumns>, Vec<RowsToProcess>), // 1. Table name, 2. Optional: Insert only for specified here column names, 3. List with rows values (which will be attached)
    CreateTable(TableName, Vec<(ColumnName, SupportedSQLDataTypes, SupportedSQLColumnConstraints)>), // 1. Table name, 2. Vector with table columns and characteristic for each column
}

/// Processing attached SQL query and returns its result as "JsonSQLTable" type ready to serialize to json format thanks to "serde" and "serde_json" crates
/// When something went bad durning analyze or processing sql query then Error without any description is returned
// Note: Polish characters are not supported by sqlparser, so not use them into queries
#[must_use = "In order to assure the best level of relaibility"]
pub fn process_sql(sql: &str, sql_action: Option<ProcessSQLSupportedQueries>) -> Result<JsonSQLTable, ()> {
    let dialect = sqlparser::dialect::AnsiDialect {};
    let parse_and_analyze_operation = sqlparser::parser::Parser::parse_sql(&dialect, sql)
        .map_or_else(
            |err| Err(()), 
            |val| Ok(val)
        )?;

    // println!("{:?}", parse_and_analyze_operation);
    let mut processed_statements: Option<JsonSQLTable> = None;
    for statement in parse_and_analyze_operation {
        match statement { // TODO: Later: rewrite from analyze "sql" param to obtaon data only "from sql_action" and after that remove "sql" param and customize code base for that
            Statement::CreateTable { 
                or_replace: _, 
                temporary: _, 
                external: _, 
                global: _, 
                if_not_exists: _, 
                name, 
                columns: columns_row, 
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
                let table_name = &name.0[0].value;
                let mut columns: Vec<JsonSQLTableColumn> = vec![];

                    // determine columns and add its to "columns" = vector with all columns for table
                for column in columns_row {
                    let col_name = column.name.value;
                        // now are supported only 2 types: Varchar and intiger given to processed sql query
                    let col_data_type = {
                        let dt = match column.data_type {
                            DataType::Varchar(len) => {
                                // when len = None is used maximum length and in JSON file VARCHAR type has assigned null // specified uinit is always expressed in bytes unit
                                let r = len
                                    .clone()
                                    .map_or_else(
                                        || SupportedSQLDataTypes::VARCHAR(None), 
                                        |val| SupportedSQLDataTypes::VARCHAR(Some(len.unwrap().length as u16))
                                    );
                                Ok(r)
                            },
                            DataType::Int(_) | DataType::Integer(_) => {
                                Ok(SupportedSQLDataTypes::INT)
                            },
                            _ => Err(())
                        };
                        dt?
                    };
                        // now is supported only NOT NULL constraint
                    let col_constraint = {
                        if column.options.len() > 0 { // for convinent is "more then 0, but it only takes 1'st option so under 0 index"
                            let c_s: Result<SupportedSQLColumnConstraints, ()> = match column.options[0].clone() { // check only first option becuase only one constraint per table is now supported
                                ColumnOptionDef { name: _, option } => {
                                    match option {
                                        ColumnOption::NotNull => {
                                            Ok(SupportedSQLColumnConstraints::NOT_NULL)
                                        },
                                        _ => Err(()) // for unusported constraints
                                    }
                                },
                                _ => Err(()) // for unusported column option type
                            };
                            let c_s = c_s?;
                            Some(
                                vec![
                                    c_s
                                ]
                            ) // KEEP IN MIND: Return vector with single option (in this moment) to avoid boilerplate in section "compose column type..."
                        }
                        else {
                            None
                        }
                    };
                        // compose column type and add it to table columns collection
                    let ready_column = JsonSQLTableColumn {
                        name: col_name,
                        d_type: col_data_type,
                        constraints: col_constraint
                    };
                    columns.push(ready_column);
                };
                    // compose sql table in json format
                let json_sql_table = JsonSQLTable {
                    name: table_name.into(),
                    columns,
                    rows: None         
                };
                    // attach computed json table from sql to returned value from whole function
                processed_statements = Some(json_sql_table);
            
            },
            Statement::Insert { 
                or: _, 
                into: _, 
                table_name: _, 
                columns: _,
                overwrite: _,
                source: _, 
                partitioned: _, 
                after_columns: _, 
                table: _,
                on: _ 
            } => {
                if sql_action.is_some() {
                    if let ProcessSQLSupportedQueries::Insert(table_path, columns, rows) = sql_action.unwrap() {
                        // TODO: Add support for columns operation
                        // TODO: Add support for constraints (e.g: When column has got NOT NULL then it must have got assigned value durning INSERT operation)
                        // TODO: Add support for autoindexing

                        // To perform operation must be minimum one row with inserted data
                        if rows.len() > 0 {
                            // Obtain already existsing table data (if it exists and is benath correct json format)
                            let table_str = if let Ok(data) = fs::read_to_string(table_path) {
                                data
                            }
                            else {
                                processed_statements = None;
                                break;
                            };
                            let mut table_json = if let Ok(json_table) = serde_json::from_str::<JsonSQLTable>(&table_str) {
                                json_table
                            }
                            else {
                                processed_statements = None;
                                break;
                            };

                            // Attach to table operation
                            let db_table_columns = &table_json.columns;
                            let db_table_rows = &mut table_json.rows;

                            let mut ready_rows = Vec::new() as Vec<Vec<JsonSQLTableColumnRow>>;

                            // Iterate over each row with data to insert into table columns
                            for row in rows {
                                // Without specified "columns" property number of columns in row must be equal to database column list 
                                if db_table_columns.len() == row.len() {
                                    // Collection with ready values to insert into table with rows. TODO: Must be checked when insert operation is processing for specific columns on angle of correct with constarints
                                    let mut ready_row_values = Vec::new() as Vec<JsonSQLTableColumnRow>;

                                    // Iterate over values to insert from one row to insert
                                    let row_len = row.len();
                                    let mut it_num: usize = 0;
                                    while it_num < row_len {
                                        let row_value = &row[it_num];
                                        let column_for_row_value = &db_table_columns[it_num];

                                        // IMPORTANT: Check types correcteness ... type must be the same as column type // + add to match!() all datatype enum tuple memebers
                                        if column_for_row_value.d_type == row_value.1 || matches!(column_for_row_value.d_type, SupportedSQLDataTypes::VARCHAR(_)) {
                                            // Additional more sophisticated type checker for more complicated types
                                            // Initialy it is always "true" so operation can be performed but in moment when type isn't correct that is changing to "false"
                                            let mut allow_to_add = true;
                                                // ... more advance checking on column datatype constraints (not same as normal constraints) 
                                            match column_for_row_value.d_type {
                                                SupportedSQLDataTypes::VARCHAR(column_t_maxlen) => {
                                                    // ... check attached value from row to column datatype
                                                    match row_value.1 {
                                                        SupportedSQLDataTypes::VARCHAR(_) => { // now always VARCHAR None
                                                            // In attached varchar type, value can't be heighter then column varchar length requirements (existing when "column_t_maxlen" is not "None")
                                                            if column_t_maxlen.is_some() && (column_t_maxlen.unwrap() < row_value.0.len() as u16 || row_value.0.len() as u16 > 65_535) { // TODO: attach on table creation that value must has got smaller length than 65_535 characters for VARCHAR datatype + attach to recognize type from query that after when string has got more then 65_535 charcters then it is no VARCHAR not TEXT (which can has got up to 16_777_215 characters)
                                                                allow_to_add = false;
                                                            };
                                                        },
                                                        _ => ()
                                                    }
                                                },
                                                _ => () // for non-special requirements
                                            };

                                            // Create ready to insert, to table value for column
                                            // Insert only when attached value has got type correct with column datatype
                                            if allow_to_add {
                                                let new_value = JsonSQLTableColumnRow {
                                                    col: column_for_row_value.name.clone(),
                                                    value: row_value.0.clone()
                                                };
                                                ready_row_values.push(new_value);
                                            }
                                            else {
                                                break;
                                            }
                                        }
                                        else {
                                            break;
                                        };

                                        it_num += 1;
                                    }
                                    
                                    // Attach row to all rows list
                                    ready_rows.push(ready_row_values);
                                }
                                else {
                                    break;
                                };
                            };

                            // Check correcteness and assign values to table "rows" key
                            if db_table_rows.is_some() && ready_rows.len() > 0 && ready_rows[0].len() == db_table_columns.len() {
                                // When already table has got saved rows
                                    // ... assign to table new rows
                                let mut db_table_rows = db_table_rows.as_mut().unwrap();
                                db_table_rows.extend(ready_rows);

                                    //... assign to table new rows
                                table_json.rows = Some(db_table_rows.clone()); // .clone() becuase i would like get rid of reference without thief whole value

                                    //... return table in json format as a result of `INSERT` operation + stop loop
                                processed_statements = Some(table_json);
                                break;
                            }
                            else if db_table_rows.is_none() && ready_rows.len() > 0 {
                                // When table hasn't got already any saved rows
                                    //... assign to table new rows
                                table_json.rows = Some(ready_rows);

                                    //... return table in json format as a result of `INSERT` operation + stop loop
                                processed_statements = Some(table_json);                            
                                break;
                            }
                        }
                        else {
                            processed_statements = None;
                            break;
                        };
                    }
                    else {
                        processed_statements = None;
                        break;
                    }
                }
                break;
            }
            _ => continue
        }
    };
    
    // When all went good (processed statement has got initialized value) then processed statement as JsonSQLTable will be returned
    match processed_statements { // this and type Option<_> for processed_statements is required by rust safegurads system
        Some(statement) => Ok(statement),
        None => Err(())
    }
}

#[test]
fn test_process_sql() {
    let computed_table = process_sql("CREATE TABLE pieski (imie_pieska varchar(2000) NOT NULL, wiek_pieska int)", None).unwrap();
    println!("Computed table is:\n{:#?}", computed_table)
}
