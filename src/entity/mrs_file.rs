use sea_orm::entity::prelude::*;

/// A converted `.mrs` rule file hosted on behalf of one token. Names are
/// unique per token: different tokens never share their files.
#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "mrs_files")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub token_id: i32,
    pub name: String,
    pub source_url: String,
    pub content: Vec<u8>,
    pub updated_at: DateTime,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
