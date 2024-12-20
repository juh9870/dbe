use crate::etype::EDataType;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphInput {
    pub ty: EDataType,
    pub id: Uuid,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphOutput {
    pub ty: EDataType,
    pub id: Uuid,
    pub name: String,
}
