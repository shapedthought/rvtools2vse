use thiserror::Error;

#[allow(dead_code)]
#[derive(Error, Debug)]
pub enum MyError {
    #[error("Could not convert enum to string: {0}")]
    EnumToString(String),
    #[error("Could not convert enum to float: {0}")]
    EnumToFloat(String),
    #[error("Could not get position of column: {0}")]
    ColumnPosition(String),
    #[error("Error with excel file")]
    ExcelError(#[from] calamine::XlsxError),
    #[error("Error with vInfo sheet")]
    VinfoError(String),
    #[error("Error with vPartition sheet")]
    VpartitionError(String),
    #[error("RvTools selection error")]
    RvtoolsError(String),
}
