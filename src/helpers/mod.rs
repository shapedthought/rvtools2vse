use calamine::{DataType, Range};

use crate::models::errors::MyError;

pub fn get_col_position(range: &Range<DataType>, col_name: &String) -> Result<usize, MyError> {
    let data_type = range.rows().next().unwrap();

    let pos = data_type
        .iter()
        .position(|x| x == &DataType::String(col_name.to_string()));

    if let Some(p) = pos {
        Ok(p)
    } else {
        Err(MyError::ColumnPosition(format!(
            "{} - {:?}",
            col_name.to_string(),
            data_type
        )))
    }
}

pub fn get_string_value(data_type: &DataType, item: String, row: usize) -> Result<String, MyError> {
    match data_type {
        DataType::String(t) => Ok(t.to_string()),
        DataType::Empty => Ok("None".to_string()),
        _ => Err(MyError::EnumToString(format!(
            "{} - row {} - Datatype {:?}",
            item, row, data_type
        ))),
    }
}

pub fn get_float_value(data_type: &DataType, item: String, row: usize) -> Result<f64, MyError> {
    match data_type {
        DataType::Float(t) => Ok(*t),
        DataType::Int(t) => Ok(*t as f64),
        DataType::Empty => Ok(0.0),
        _ => Err(MyError::EnumToFloat(format!(
            "{} - row {} - Datatype {:?}",
            item, row, data_type
        ))),
    }
}
