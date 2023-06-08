use calamine::{DataType, Range};

use crate::models::errors::MyError;

pub trait ColPosition {
    fn get_col_pos(&self, col_name: &String) -> Result<usize, MyError>;
}

impl ColPosition for Range<DataType> {
    fn get_col_pos(&self, col_name: &String) -> Result<usize, MyError> {
        let data_type = &self.rows().next().unwrap();

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
}

pub trait GetString {
    fn get_string_value(&self, item: String, row: usize) -> Result<String, MyError>;
}

impl GetString for DataType {
    fn get_string_value(&self, item: String, row: usize) -> Result<String, MyError> {
        match self {
            DataType::String(t) => Ok(t.to_string()),
            DataType::Empty => Ok("None".to_string()),
            _ => Err(MyError::EnumToString(format!(
                "{} - row {} - Datatype {:?}",
                item, row, self
            ))),
        }
    }
}

pub trait GetFloat {
    fn get_float_value(&self, item: String, row: usize) -> Result<f64, MyError>;
}

impl GetFloat for DataType {
    fn get_float_value(&self, item: String, row: usize) -> Result<f64, MyError> {
        match self {
            DataType::Float(t) => Ok(*t),
            DataType::Int(t) => Ok(*t as f64),
            DataType::Empty => Ok(0.0),
            _ => Err(MyError::EnumToFloat(format!(
                "{} - row {} - Datatype {:?}",
                item, row, self
            ))),
        }
    }
}
