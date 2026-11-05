use calamine::{Data, Range};

use crate::models::errors::MyError;

pub trait ColPosition {
    fn get_col_pos(&self, col_name: &String) -> Result<usize, MyError>;
}

impl ColPosition for Range<Data> {
    fn get_col_pos(&self, col_name: &String) -> Result<usize, MyError> {
        let data_type = &self.rows().next().unwrap();

        let pos = data_type
            .iter()
            .position(|x| x == &Data::String(col_name.to_string()));

        if let Some(p) = pos {
            Ok(p)
        } else {
            Err(MyError::ColumnPosition(format!("{}", col_name.to_string())))
        }
    }
}

pub trait GetString {
    fn get_string_value(&self, item: String, row: usize) -> Result<String, MyError>;
}

impl GetString for Data {
    fn get_string_value(&self, item: String, row: usize) -> Result<String, MyError> {
        match self {
            Data::String(t) => Ok(t.to_string()),
            Data::Empty => Ok("None".to_string()),
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

impl GetFloat for Data {
    fn get_float_value(&self, item: String, row: usize) -> Result<f64, MyError> {
        match self {
            Data::Float(t) => Ok(*t),
            Data::Int(t) => Ok(*t as f64),
            Data::Empty => Ok(0.0),
            _ => Err(MyError::EnumToFloat(format!(
                "{} - row {} - Datatype {:?}",
                item, row, self
            ))),
        }
    }
}
