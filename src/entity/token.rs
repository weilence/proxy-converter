use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "tokens")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    #[sea_orm(unique)]
    pub token: String,
    /// Credential embedded in the config's hosted-file URLs; separate from
    /// `token` so a leaked config does not expose the subscription token.
    #[sea_orm(unique)]
    pub file_key: String,
    pub name: String,
    pub config: String,
    pub enabled: bool,
    pub expires_at: Option<DateTime>,
    pub created_at: DateTime,
    pub last_used_at: Option<DateTime>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
