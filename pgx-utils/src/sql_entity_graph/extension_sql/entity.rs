use crate::sql_entity_graph::{
    extension_sql::SqlDeclared, pgx_sql::PgxSql, positioning_ref::PositioningRef, to_sql::ToSql,
    SqlGraphEntity, SqlGraphIdentifier,
};

use std::fmt::Display;

/// The output of a [`ExtensionSql`](crate::sql_entity_graph::ExtensionSql) from `quote::ToTokens::to_tokens`.
#[derive(Debug, Clone, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct ExtensionSqlEntity {
    pub module_path: &'static str,
    pub full_path: &'static str,
    pub sql: &'static str,
    pub file: &'static str,
    pub line: u32,
    pub name: &'static str,
    pub bootstrap: bool,
    pub finalize: bool,
    pub requires: Vec<PositioningRef>,
    pub creates: Vec<SqlDeclaredEntity>,
}

impl ExtensionSqlEntity {
    pub fn has_sql_declared_entity(&self, identifier: &SqlDeclared) -> Option<&SqlDeclaredEntity> {
        self.creates
            .iter()
            .find(|created| created.has_sql_declared_entity(identifier))
    }
}

impl Into<SqlGraphEntity> for ExtensionSqlEntity {
    fn into(self) -> SqlGraphEntity {
        SqlGraphEntity::CustomSql(self)
    }
}

impl SqlGraphIdentifier for ExtensionSqlEntity {
    fn dot_identifier(&self) -> String {
        format!("sql {}", self.name)
    }
    fn rust_identifier(&self) -> String {
        self.name.to_string()
    }

    fn file(&self) -> Option<&'static str> {
        Some(self.file)
    }

    fn line(&self) -> Option<u32> {
        Some(self.line)
    }
}

impl ToSql for ExtensionSqlEntity {
    #[tracing::instrument(level = "debug", skip(self, _context), fields(identifier = self.full_path))]
    fn to_sql(&self, _context: &PgxSql) -> eyre::Result<String> {
        let sql = format!(
            "\n\
                -- {file}:{line}\n\
                {bootstrap}\
                {creates}\
                {requires}\
                {finalize}\
                {sql}\
                ",
            file = self.file,
            line = self.line,
            bootstrap = if self.bootstrap { "-- bootstrap\n" } else { "" },
            creates = if !self.creates.is_empty() {
                format!(
                    "\
                    -- creates:\n\
                    {}\n\
                ",
                    self.creates
                        .iter()
                        .map(|i| format!("--   {}", i))
                        .collect::<Vec<_>>()
                        .join("\n")
                ) + "\n"
            } else {
                "".to_string()
            },
            requires = if !self.requires.is_empty() {
                format!(
                    "\
                   -- requires:\n\
                    {}\n\
                ",
                    self.requires
                        .iter()
                        .map(|i| format!("--   {}", i))
                        .collect::<Vec<_>>()
                        .join("\n")
                ) + "\n"
            } else {
                "".to_string()
            },
            finalize = if self.finalize { "-- finalize\n" } else { "" },
            sql = self.sql,
        );
        tracing::trace!(%sql);
        Ok(sql)
    }
}

#[derive(Debug, Clone, Hash, PartialEq, Eq, Ord, PartialOrd)]
pub struct SqlDeclaredEntityData {
    sql: String,
    name: String,
    option: String,
    vec: String,
    vec_option: String,
    option_vec: String,
    option_vec_option: String,
    array: String,
    option_array: String,
    varlena: String,
    pg_box: Vec<String>,
}
#[derive(Debug, Clone, Hash, PartialEq, Eq, Ord, PartialOrd)]
pub enum SqlDeclaredEntity {
    Type(SqlDeclaredEntityData),
    Enum(SqlDeclaredEntityData),
    Function(SqlDeclaredEntityData),
}

impl Display for SqlDeclaredEntity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SqlDeclaredEntity::Type(data) => {
                f.write_str(&(String::from("Type(") + &data.name + ")"))
            }
            SqlDeclaredEntity::Enum(data) => {
                f.write_str(&(String::from("Enum(") + &data.name + ")"))
            }
            SqlDeclaredEntity::Function(data) => {
                f.write_str(&(String::from("Function ") + &data.name + ")"))
            }
        }
    }
}

impl SqlDeclaredEntity {
    pub fn build(variant: impl AsRef<str>, name: impl AsRef<str>) -> eyre::Result<Self> {
        let name = name.as_ref();
        let data = SqlDeclaredEntityData {
            sql: name
                .split("::")
                .last()
                .ok_or_else(|| eyre::eyre!("Did not get SQL for `{}`", name))?
                .to_string(),
            name: name.to_string(),
            option: format!("Option<{}>", name),
            vec: format!("Vec<{}>", name),
            vec_option: format!("Vec<Option<{}>>", name),
            option_vec: format!("Option<Vec<{}>>", name),
            option_vec_option: format!("Option<Vec<Option<{}>>", name),
            array: format!("Array<{}>", name),
            option_array: format!("Option<{}>", name),
            varlena: format!("Varlena<{}>", name),
            pg_box: vec![
                format!("pgx::pgbox::PgBox<{}>", name),
                format!("pgx::pgbox::PgBox<{}, pgx::pgbox::AllocatedByRust>", name),
                format!(
                    "pgx::pgbox::PgBox<{}, pgx::pgbox::AllocatedByPostgres>",
                    name
                ),
            ],
        };
        let retval = match variant.as_ref() {
            "Type" => Self::Type(data),
            "Enum" => Self::Enum(data),
            "Function" => Self::Function(data),
            _ => {
                return Err(eyre::eyre!(
                    "Can only declare `Type(Ident)`, `Enum(Ident)` or `Function(Ident)`"
                ))
            }
        };
        Ok(retval)
    }
    pub fn sql(&self) -> String {
        match self {
            SqlDeclaredEntity::Type(data) => data.sql.clone(),
            SqlDeclaredEntity::Enum(data) => data.sql.clone(),
            SqlDeclaredEntity::Function(data) => data.sql.clone(),
        }
    }

    pub fn has_sql_declared_entity(&self, identifier: &SqlDeclared) -> bool {
        match (&identifier, &self) {
            (SqlDeclared::Type(identifier_name), &SqlDeclaredEntity::Type(data))
            | (SqlDeclared::Enum(identifier_name), &SqlDeclaredEntity::Enum(data))
            | (SqlDeclared::Function(identifier_name), &SqlDeclaredEntity::Function(data)) => {
                let matches = |identifier_name: &str| {
                    identifier_name == &data.name
                        || identifier_name == &data.option
                        || identifier_name == &data.vec
                        || identifier_name == &data.vec_option
                        || identifier_name == &data.option_vec
                        || identifier_name == &data.option_vec_option
                        || identifier_name == &data.array
                        || identifier_name == &data.option_array
                        || identifier_name == &data.varlena
                };
                if matches(&*identifier_name) || data.pg_box.contains(&identifier_name) {
                    return true;
                }
                // there are cases where the identifier is
                // `core::option::Option<Foo>` while the data stores
                // `Option<Foo>` check again for this
                let generics_start = match identifier_name.find('<') {
                    None => return false,
                    Some(idx) => idx,
                };
                let qualification_end = match identifier_name[..generics_start].rfind("::") {
                    None => return false,
                    Some(idx) => idx,
                };
                matches(&identifier_name[qualification_end + 2..])
            }
            _ => false,
        }
    }
}
