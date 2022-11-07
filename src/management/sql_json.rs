#![allow(unused)]
use std::{fs, path::{Path, PathBuf}, collections::HashMap};

use serde::{self, Deserialize, Serialize};
use sqlparser::{
    self,
    ast::{ColumnOption, ColumnOptionDef, DataType, Statement, Expr, Value as SQLParserValue, BinaryOperator},
};
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
    pub rows: Option<Vec<Vec<JsonSQLTableColumnRow>>>,
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
    BOOLEAN,
}

/// Trait to easy and seamlessly convertion between SQLParser Types stored benath "DataTypes" enum and supported types by database
pub trait ConvertSQLParserTypesToSupported {
    fn convert(parser_type: &DataType) -> Option<SupportedSQLDataTypes> {
        use DataType::*;
        // TODO: Add support for more types
        match parser_type {
            Varchar(prop) => {
                if let Some(prop) = prop {
                    let len = prop.length;
                    Some(SupportedSQLDataTypes::VARCHAR(Some(len as u16))) // u16 is sufficient becuse varchar can store maximum 65_535 characters
                }
                else {
                    Some(SupportedSQLDataTypes::VARCHAR(None))
                }
            },
            Int(_width) => { // TODO: add support for Int width
                Some(SupportedSQLDataTypes::INT)
            },
            Text => {
                Some(SupportedSQLDataTypes::TEXT)
            },
            _ => None // unsuported
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[allow(non_camel_case_types)]
/// Represent all supported column constraints in this database
pub enum SupportedSQLColumnConstraints {
    PRIMARY_KEY,
    FOREGIN_KEY,
    DEFAULT(String),
    NOT_NULL,
    UNIQUE,
}

/// Trait to easy and seamlessly convertion between SQLParser Options (in sqlparser and is equal concept to Constraint) stored benath "ColumnOption.option" as a "ColumnOption" enum to supported constraints by database
pub trait ConvertSQLParserOptionsToSupportedConstraints {
    fn convert(option: sqlparser::ast::ColumnOptionDef) -> Option<SupportedSQLColumnConstraints> {
        use sqlparser::ast::ColumnOption::*;
        // TODO: Add support for more constraints
        match option.option {
            NotNull => Some(SupportedSQLColumnConstraints::NOT_NULL),
            _ => None // for unsuppored options
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
/// Represent each column placed in JsonSQLTable
pub struct JsonSQLTableColumn {
    /// column name
    pub name: String,
    /// column data type
    pub d_type: SupportedSQLDataTypes,
    /// optional column constraints
    pub constraints: Option<Vec<SupportedSQLColumnConstraints>>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
/// Represent each row with data for "JsonSQLTable" struct
pub struct JsonSQLTableColumnRow {
    pub col: String,
    pub value: Option<String>,
}

#[derive(Debug, PartialEq)]
pub struct ProcessSQLRowField(pub String, pub SupportedSQLDataTypes); // 1. field value, 2. Field data type (only supported datatypes)

type TableName = String;
type TablePath<'x> = &'x PathBuf;
type ColumnName = String;
type ActionOnlyForTheseColumns = Vec<ColumnName>;
type RowsToProcess = Vec<ProcessSQLRowField>; 

#[derive(Debug)]
/// Includes all "INSERT" operation mutations 
pub enum InsertOperations {
    Into,
    Overwrite
}

#[derive(Debug)]
pub enum ProcessSQLSupportedQueries<'x> {
    Insert(
        TablePath<'x>,
        Option<ActionOnlyForTheseColumns>,
        Vec<RowsToProcess>,
        InsertOperations
    ), // 1. Table name, 2. Optional: Insert only for specified here column names, 3. List with rows values (which will be attached), 4. Operation type
    CreateTable(
        TableName,
        Vec<(
            ColumnName,
            SupportedSQLDataTypes,
            Option<Vec<SupportedSQLColumnConstraints>>,
        )>,
    ), // 1. Table name, 2. Vector with table columns and characteristic for each column
    Truncate(TablePath<'x>),
    Select(TablePath<'x>, ActionOnlyForTheseColumns, Option<Expr>) // path to table, 2. return results for specific record tuples can be all, 3. Select only these records
}

#[derive(Debug, Clone)]
/// Describe operation for row
struct RowWhereOperation {
    /// column name
    column: Option<String>,
    /// column value
    value: Option<String>,
    /// type is from "sqlparser" crate
    op: BinaryOperator,
    /// in match operation indicates whether operation has been successfullperformed
    perf: Option<bool>
}

/// Processing attached SQL query and returns its result as "JsonSQLTable" type ready to serialize, to json format thanks to "serde" and "serde_json" crates
/// When something went bad durning analyze or processing sql query then Error without any description is returned
// Note: Polish characters are not supported by sqlparser, so not use them into queries
#[must_use = "In order to assure the best level of relaibility"]
pub fn process_sql(sql_action: ProcessSQLSupportedQueries) -> Result<JsonSQLTable, ()> {
    use ProcessSQLSupportedQueries::*;
    match sql_action { // only operations which require changes/obtain data/mainupulate file content in any manner
        CreateTable(table_name, columns) => {
            if columns.len() > 0 { // can be treat as boilerplate but i feel safier with this statement
                let mut ready_columns: Vec<JsonSQLTableColumn> = vec![];

                // determine columns and add its to "columns" = vector with all columns for table
                for column in columns {
                    let col_name = column.0;
                    // process specific datatype to appropriate form or maintain other
                    let col_data_type = {
                        match column.1 {
                            SupportedSQLDataTypes::VARCHAR(len) => {
                                // when len = None is used maximum length and in JSON file VARCHAR type has assigned null // specified uinit is always expressed in bytes unit
                                len.map_or_else(
                                    || SupportedSQLDataTypes::VARCHAR(None), // maximum value for varchar is 65535 characters
                                    |val| SupportedSQLDataTypes::VARCHAR(Some(val)),
                                )
                            },
                            _ => column.1,
                        }
                    };
                    // attach constraints
                    let col_constraint = {
                        if column.2.is_some() && column.2.clone().unwrap().len() >= 1 {
                            Some(column.2.unwrap())
                        }
                        else {
                            None
                        }
                    };
                    // compose column type and add it to table columns collection
                    let ready_column = JsonSQLTableColumn {
                        name: col_name,
                        d_type: col_data_type,
                        constraints: col_constraint,
                    };
                    ready_columns.push(ready_column);
                }
                // compose sql table in json format
                let json_sql_table = JsonSQLTable {
                    name: table_name.into(),
                    columns: ready_columns,
                    rows: None,
                };
                // attach computed json table from sql to returned value from whole function
                Ok(json_sql_table)
            }
            else {
                Err(())
            }
        }
        Insert(table_path, columns, rows, op_type) => {
            // TODO: Add support for When column type is different then this inffered for query collumn but format of value should be supported like between: "Varchar" and "TEXT" type
            // TODO: Add support for constraints (e.g: When column has got NOT NULL then it must have got assigned value durning INSERT operation)
            // TODO: Add support for autoindexing
            // TODO: Better system to checking types inside this method (number can't be asigned to string)

            // To perform operation must be minimum one row with inserted data
            if rows.len() > 0 {
                // Obtain already existsing table data (if it exists and is benath correct json format)
                let table_str = if let Ok(data) = fs::read_to_string(table_path) {
                    data
                } else {
                    return Err(());
                };
                let mut table_json =
                    if let Ok(json_table) = serde_json::from_str::<JsonSQLTable>(&table_str) {
                        json_table
                    } else {
                        return Err(());
                    };

                // Attach to table operation
                let db_table_columns = &table_json.columns;
                let db_table_rows = &mut table_json.rows;

                // When columns to which values should be inserted were attached then check whether addition for specific columns can be performed
                // When columns weren't attached then ignore this code block
                let mut existing_columns_to_perform_list: std::collections::HashMap<String, JsonSQLTableColumn> = HashMap::new();
                let mut existing_columns_to_perform: Vec<String> = vec![];
                let mut columns_not_included_in_query: Vec<&JsonSQLTableColumn> = vec![];
                if columns.is_some() {
                    let columns = columns.clone().unwrap();
                    
                    // check whether all columns given into query exists and put this column into Vector
                    for column_perf_for in &columns {
                        let databse_has_column = db_table_columns
                            .iter()
                            .enumerate()
                            .any(|val| {
                                if &val.1.name == column_perf_for {
                                    true
                                }
                                else {
                                    false
                                }
                            });
                        
                        if databse_has_column {
                            let column = db_table_columns
                                .iter()
                                .find(|col| {
                                    if &col.name == column_perf_for {
                                        return true
                                    };
                                    
                                    false
                                });
                            
                            if let Some(col) = column {
                                existing_columns_to_perform.push(column_perf_for.clone());
                                existing_columns_to_perform_list.insert(column_perf_for.clone(), col.clone());
                            }
                            else {
                                break;
                            }
                        }
                        else {
                            break;
                        };
                    };

                    // Go further only when all columns from query exists in table and below checking has been done to advantage of "perform further"
                    // When all is correct after check then perform further, else return Err(())
                    if existing_columns_to_perform.len() == columns.len() {
                        // Check whether remained column doesn't have constraints "not null"
                            // ... obtain all reamained columns + assign it to scope range variable
                        columns_not_included_in_query = db_table_columns.
                            iter()
                            .filter(|column| {
                                if !existing_columns_to_perform.contains(&column.name) {
                                    true
                                }
                                else {
                                    false
                                }
                            })
                            .collect::<Vec<&JsonSQLTableColumn>>();
                        
                            // ... Check whether all remained columns so (these "not included in query") not inclueded constraint "NOT NULL" (when not includes then result is "true")
                        let all_remained_dn_null = columns_not_included_in_query // indicate whether all columns doesn't have NOT_NULL constraint
                            .iter()
                            .all(|remained_column| {
                                if let Some(constraints_vec) = &remained_column.constraints {
                                    match &remained_column.constraints {
                                        Some(constraints_vec) => {
                                            // When vector is empty that NOT_NULL constraint doesn't exists so return "false"
                                            if constraints_vec.len() > 0 {
                                                // Check whether in vector with constraints is any NOT_NULL constraint (when is return "false" when is that constraint (from this reason "not" operator begin statement!))
                                                constraints_vec
                                                    .iter()
                                                    .any(|constraint| {
                                                        match constraint.clone() {
                                                            SupportedSQLColumnConstraints::NOT_NULL => false,
                                                            _ => true
                                                        }
                                                    })
                                            }
                                            else {
                                                true
                                            }
                                        },
                                        None => true
                                    }
                                }
                                else {
                                    true
                                }
                            });

                        // When some from remained column contains NOT_NULL constraiint then return Err(()) 
                        if !all_remained_dn_null { 
                            return Err(());
                        }
                        // else ... Go further and perform addition
                    }
                    else {
                        return Err(());
                    }
                }

                // Ready rows to insert into table
                let mut ready_rows = Vec::new() as Vec<Vec<JsonSQLTableColumnRow>>;

                // Iterate over each row with data to insert into table columns. Inside among others are checking row type correctensess respect to column type
                for row in rows {
                    // Always no matter upon operation type columns len must be equal to list of values in row 
                    if db_table_columns.len() == row.len() || (columns.is_some() && row.len() == columns.clone().unwrap().len()) {
                        // Collection with ready values to insert into table with rows
                        let mut ready_row_values = Vec::new() as Vec<JsonSQLTableColumnRow>;

                        // Iterate over values to insert from one row to insert
                        let row_len = row.len();
                        let mut it_num: usize = 0;
                        while it_num < row_len {
                            let row_value = &row[it_num];
                            let column_for_row_value = {
                                if columns.is_none() { // column for normal addition
                                    if db_table_columns.len() > it_num {
                                        &db_table_columns[it_num]
                                    }
                                    else {
                                        break;
                                    }
                                }
                                else { // column for addition for specific columns
                                    if existing_columns_to_perform.len() > it_num {
                                        // Because columns len must be equal to len of values in row so always Some(val)
                                        if let Some(val) = existing_columns_to_perform_list.get(&existing_columns_to_perform[it_num]) {
                                            val
                                        }
                                        else {
                                            break;
                                        }
                                    }
                                    else {
                                        break;
                                    }
                                }
                            };

                            // IMPORTANT: Check types correcteness ... type must be the same as column type // + add to match!() all datatype enum tuple memebers
                            if column_for_row_value.d_type == row_value.1
                                || matches!(
                                    column_for_row_value.d_type,
                                    SupportedSQLDataTypes::VARCHAR(_)
                                )
                                || (column_for_row_value.d_type == SupportedSQLDataTypes::TEXT && matches!(row_value.1, SupportedSQLDataTypes::VARCHAR(_))) // "VARCHAR" should also be added to columns with "TEXT" type (because varchat capacity is smaller then TEXT)
                            {
                                // Additional more sophisticated type checker for more complicated types
                                // Initialy it is always "true" so operation can be performed but in moment when type isn't correct that is changing to "false"
                                let mut allow_to_add = true;
                                // ... more advance checking on column datatype constraints (not same as normal constraints)
                                match column_for_row_value.d_type {
                                    SupportedSQLDataTypes::VARCHAR(column_t_maxlen) => {
                                        // ... check attached value from row to column datatype
                                        match row_value.1 {
                                            SupportedSQLDataTypes::VARCHAR(_) => {
                                                // now always VARCHAR None
                                                // In attached varchar type, value can't be heighter then column varchar length requirements (existing when "column_t_maxlen" is not "None")
                                                if column_t_maxlen.is_some()
                                                    && (column_t_maxlen.unwrap()
                                                        < row_value.0.len() as u16
                                                        || row_value.0.len() as u16 > 65_535)
                                                {
                                                    // TODO: attach on table creation that value must has got smaller length than 65_535 characters for VARCHAR datatype + attach to recognize type from query that after when string has got more then 65_535 charcters then it is no VARCHAR not TEXT (which can has got up to 16_777_215 characters)
                                                    allow_to_add = false;
                                                };
                                            }
                                            _ => (),
                                        }
                                    }
                                    _ => (), // for non-special requirements
                                };

                                // Create ready to insert, to table value for column
                                // For whole columns insert: Insert only when attached value has got type correct with column datatype
                                // For insert for specific columns: Insert value for specific column and full fill remained columns with values null
                                if allow_to_add {
                                    // insert normal value
                                    let new_value = JsonSQLTableColumnRow {
                                        col: column_for_row_value.name.clone(),
                                        value: Some(row_value.0.clone()),
                                    };
                                    ready_row_values.push(new_value);

                                    // Add to row values for remained columns with attached "null" as a value
                                    // below instruction ignore type safeguards ("null" -> can be attached to all keys which doesn't have got NOT_NULL constraint)
                                    if it_num == row_len - 1 && columns.is_some() {
                                        let mut remained_row_values = vec![] as Vec<JsonSQLTableColumnRow>;

                                        for colmn_out_from_query in &columns_not_included_in_query {
                                            let remained_row_value = JsonSQLTableColumnRow {
                                                col: colmn_out_from_query.name.to_owned(),
                                                value: None
                                            };
                                            remained_row_values.push(remained_row_value);
                                        };
                                        ready_row_values.extend(remained_row_values);
                                    };
                                } else {
                                    break;
                                }
                            } else {
                                break;
                            };

                            it_num += 1;
                        }

                        // Attach row to all rows list // Always great result
                        ready_rows.push(ready_row_values);
                    } else {
                        break;
                    };
                }

                // Check correcteness and assign values to table "rows" key
                if db_table_rows.is_some()
                    && ready_rows.len() > 0
                    && ready_rows[0].len() == db_table_columns.len()
                    && matches!(op_type, InsertOperations::Into)
                {
                    // When already table has got saved rows
                    // ... assign to table new rows
                    let mut db_table_rows = db_table_rows.as_mut().unwrap();
                    db_table_rows.extend(ready_rows);

                    //... assign to table new rows
                    table_json.rows = Some(db_table_rows.clone()); // .clone() becuase i would like get rid of reference without thief whole value

                    //... return table in json format as a result of `INSERT` operation + stop loop
                    return Ok(table_json);
                } else if ready_rows.len() > 0
                    && ready_rows[0].len() == db_table_columns.len() 
                    && ((db_table_rows.is_none() && ready_rows.len() > 0) 
                        || (matches!(op_type, InsertOperations::Overwrite) && ready_rows.len() > 0)) 
                {
                    println!("i");
                    // When table hasn't got already any saved rows or INSERT OPERATION has been characterized as "INSERT OVERWRITE TABLE"
                    //... assign to table new rows
                    table_json.rows = Some(ready_rows);

                    //... return table in json format as a result of `INSERT` operation + stop loop
                    return Ok(table_json);
                }

                return Err(()); // otherwise (is returned for example when "ready_rows.len() != db_table_columns.len()" which occurs when row type isn't equal to type specified for column)
            } else {
                return Err(());
            }
        },
        Truncate(table_path) => {
            // To perform whole operation: specified table must exists, table must be in JSON format before serialization. Else "Err(())" is returned
            // Check whether path exists isn't perform here!
            if let Ok(table_str) = fs::read_to_string(table_path) {
                let table_json = serde_json::from_str::<JsonSQLTable>(&table_str);
                if table_json.is_ok() {
                    let mut table_json = table_json.unwrap();
                    table_json.rows = None;
                    
                    Ok(table_json)
                }
                else {
                    Err(())
                }
            }
            else {
                Err(())
            }
        },
        Select(table_path, resulting_columns, conditions) => {
            let table_data = fs::read_to_string(table_path).unwrap();
            let mut json_t_data = serde_json::from_str::<JsonSQLTable>(&table_data).unwrap(); // I trust other Database functionalities to maintain correct JSON format
            let t_d_rows = &json_t_data.rows; // WARNING: for simply access but not for assign values!!!

            if t_d_rows.is_some() {
                let t_d_rows = t_d_rows.as_ref().unwrap();
                if t_d_rows.len() > 0 {
                    // Get whether user pass columns which are into table or pass "all" option (for return all columns)
                    let table_col_names = json_t_data.columns.iter()
                        .enumerate()
                        .filter_map(|col| {
                          Some(&col.1.name)
                       })
                        .collect::<Vec<&String>>();
                    // Whether user add table column names or appropraite option
                    let user_pass_table_cols = resulting_columns.iter()
                        .enumerate()
                        .all(|col_to_ret| {
                            let col_name = col_to_ret.1;
                        
                            if table_col_names.contains(&col_name) || col_name == &"all".to_string() {
                                return true;
                            };

                            false
                        });

                    // TODO: Make conditions given after 'WHERE' usable (if were putted)
                    if let Some(expr_conditions) = conditions {
                        // list with converted expressions from 'WHERE'
                        let mut operations_for_row: Vec<RowWhereOperation> = Vec::new(); // [{ column: Some("gender"), value: Some("male"), op: Eq }, { op: And, column: None, value: None }]

                        /// Function for convert Expr::BinOp to RowWhereOperation expression and put it into "operations_for_row" collection to facilitate performant 'WHERE' computing
                        fn convert_binarop(expr: Expr, converted_list: &mut Vec<RowWhereOperation>) -> Result<(), ()> {
                            if let Expr::BinaryOp { left, op, right } = expr { // for parent
                                /// To convert expression witch doesn't rollup further to conjuction (And, Or) 
                                fn for_value_and_column(op_row_collection: &mut Vec<RowWhereOperation>, right: &Box<Expr>, left: &Box<Expr>, op: &BinaryOperator) -> Result<(), ()> {
                                    let no_rollup_cond = RowWhereOperation {
                                        column: { // column "name"
                                            match &**left {
                                                Expr::Identifier(d) => {
                                                    Some(d.value.clone())
                                                },
                                                _ => return Err(()) // incorrect parsed condition
                                            }
                                        },
                                        op: op.clone(), // operation type like: Eq, NotEq, Less , ...
                                        value: { // column "value"
                                            match &**right {
                                                Expr::Identifier(d) => {
                                                    Some(d.value.clone())
                                                },
                                                Expr::Value(value) => {
                                                    match value {
                                                        SQLParserValue::SingleQuotedString(sval) | SQLParserValue::DoubleQuotedString(sval) => Some(sval.to_owned()),
                                                        SQLParserValue::Number(num, _) => {
                                                            Some(num.clone())
                                                        },
                                                        SQLParserValue::Boolean(boolval) => Some(boolval.to_string()),
                                                        SQLParserValue::Null => Some(String::from("null")),
                                                        _ => return Err(())
                                                    }
                                                },
                                                _ => return Err(()) // incorrect parsed condition
                                            }
                                        },
                                        perf: None
                                    };

                                    // Add condition part to list
                                    op_row_collection.push(no_rollup_cond);

                                    // Performed indicator result
                                    Ok(())
                                }
                                
                                /// Appropriate action to appropriate outcome
                                match op {
                                    BinaryOperator::And | BinaryOperator::Or => { // for multiple blocks // in that case "left" and "right" keys allways represents next "BinaryOp" struct
                                        // left
                                        convert_binarop(*left, converted_list)?;

                                        // Add conjuction
                                        let conjuction = RowWhereOperation {
                                            column: None,
                                            value: None,
                                            op: op.clone(),
                                            perf: None
                                        };
                                        converted_list.push(conjuction);

                                        // right
                                        convert_binarop(*right, converted_list)?;

                                        // Result
                                        Ok(())
                                    }, 
                                    _ => for_value_and_column(converted_list, &right, &left, &op) // for row operations   
                                }
                            }
                            else {
                                // No-predicted behave
                                Err(())
                            }
                        }

                        // Convert whole to expected form
                        convert_binarop(expr_conditions, &mut operations_for_row)?;

                        let mut s_rows = Vec::new() as Vec<Vec<JsonSQLTableColumnRow>>;
                        let mut op_performed_whole = true; // when false result shoudn't be returned and search operation performed further
                        
                        // Iterate over conditions and try to find appropriate columns
                        let mut it_op_id = 0;
                        loop {
                            if it_op_id < operations_for_row.len() && op_performed_whole {
                                // get condition to later match
                                let rm = operations_for_row.clone(); // to easy compare in And, Or conditions
                                let op_for_row = &mut operations_for_row[it_op_id];

                                // Src operation trashold:
                                let sc_name = op_for_row.column.clone();
                                let sc_val = op_for_row.value.clone();
                                let mut match_found: bool = false;
    
                                //... Comparing clousure // op: "Eq"/"Less" etc...
                                let mut search_match_in_row = || {
                                    for row in t_d_rows {
                                        for row_vals in row {
                                            // Perform specific action abd add positive match result to results list
                                            match op_for_row.op {
                                                BinaryOperator::Eq => {
                                                    if &row_vals.col == sc_name.as_ref().unwrap() && row_vals.value == sc_val {
                                                        match_found = true; // match is here so indicate other members about that
                                                        op_for_row.perf = Some(true); // indicate that operation has been successfull performed
                                                        s_rows.push(row.clone());
                                                        break;
                                                    }
                                                },
                                                _ => () // no handled
                                            }
                                        }
                                    }

                                    // For consistancy: when match wasn't found (cohestive working should be represented in this way)
                                    if !match_found {
                                        op_for_row.perf = Some(false);
                                    }
                                };
    
                                // And/Or conditions pissed here
                                if let BinaryOperator::And = op_for_row.op {
                                    if rm[it_op_id - 1].perf.is_some() && rm[it_op_id - 1].perf.unwrap() {
                                        // lead operation further
                                        search_match_in_row();
                                    }
                                    else {
                                        // stop operation in for e.g in case like: previous search operation ends with "false" result because whatever row hasn't been matched to condition 
                                        op_performed_whole = false; // stop operation further in operation stage
                                        break; // stop operation further locally
                                    }
                                }
                                else {
                                    // Lead further for other "op" types like Eq/Gt/Less etc...
                                    search_match_in_row();

                                    // When result hasn't been matched in any row by above clousure (enclosed in brackets "{}")
                                    if !match_found {
                                        if let BinaryOperator::And = rm[it_op_id - 1].op {
                                            op_performed_whole = false; // because all conditions between "AND" statement must results in match found (outcome "true")
                                        }
                                        // TODO: BinaryOperator::Or
                                    }
                                }

                                // increase iterated elements colunt
                                it_op_id += 1;
                            }
                            else {
                                break;
                            }
                        };
                        println!("{:#?} {}", s_rows, op_performed_whole);
                    }

                        // Go ahead only when user pass table column names or "all" option
                    if user_pass_table_cols {
                        // Return only fields for columns which user would like to get
                        if resulting_columns[0] == "all" {
                            // Return all columns for matched records
                            json_t_data.rows = Some(t_d_rows.to_owned());
                            return Ok(json_t_data);
                        }
                        else {
                            // Return only fields for columns which user would like to get 
                            let mut f_results = vec![] as Vec<Vec<JsonSQLTableColumnRow>>;
                            for row in t_d_rows.clone() {
                                let mut row_passed_fields_ready = vec![] as Vec<JsonSQLTableColumnRow>;
                                let _ = row
                                    .iter()
                                    .enumerate()
                                    .filter(|field| {
                                        let f_d = field.1;

                                        if resulting_columns.contains(&f_d.col) {
                                            return true
                                        };

                                        return false
                                    })
                                    .collect::<Vec<(usize, &JsonSQLTableColumnRow)>>()
                                    .into_iter()
                                    .for_each(|record| {
                                        row_passed_fields_ready.push(record.1.to_owned())
                                    });
                                f_results.push(row_passed_fields_ready)
                            }

                            json_t_data.rows = Some(f_results);
                            return Ok(json_t_data);
                        }
                    }
                    else {
                        return Err(())
                    }
                }
                else {
                    // Return table withput rows // with null benath "rows" key
                    json_t_data.rows = None; // Return null for "rows" but not empty array. "serde_json" threat that as null in json file
                    return Ok(json_t_data)
                }
            }
            else {
                // Return table withput rows // with null benath "rows" key
                return Ok(json_t_data)
            }
        }
    }
}

#[test]
fn test_process_sql() {
    let tab_name = "new_table".to_string();
    let row1 = (String::from("imie"), SupportedSQLDataTypes::VARCHAR(Some(12)), Some(vec![SupportedSQLColumnConstraints::NOT_NULL]));
    let row2 = (String::from("imie"), SupportedSQLDataTypes::INT, None);
    let computed_table = process_sql(ProcessSQLSupportedQueries::CreateTable(tab_name, vec![row1, row2])).unwrap();
    let serialized = serde_json::to_string(&computed_table).unwrap();
    println!("Computed table is:\n{}", serialized)
}
