use serde::{Serialize, Serializer};

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("migration error: {0}")]
    Migration(#[from] rusqlite_migration::Error),
    #[error("keystore error: {0}")]
    Keystore(String),
    #[error("the database is locked; unlock it before using this command")]
    Locked,
    #[error("invalid input: {0}")]
    Invalid(String),
    #[error("simulation parameters are invalid")]
    SimParams(Vec<crate::sim::ParamError>),
    #[error("network request failed: {0}")]
    Network(String),
    #[error("{0}")]
    Other(String),
}

pub type AppResult<T> = Result<T, AppError>;

/// Tauri requires command errors to be serialisable. Errors are surfaced to the
/// UI as a tagged object so the frontend can branch on `kind` without parsing
/// message strings.
impl Serialize for AppError {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let kind = match self {
            AppError::Db(_) => "db",
            AppError::Migration(_) => "migration",
            AppError::Keystore(_) => "keystore",
            AppError::Locked => "locked",
            AppError::Invalid(_) => "invalid",
            AppError::SimParams(_) => "sim_params",
            AppError::Network(_) => "network",
            AppError::Other(_) => "other",
        };
        let mut st = s.serialize_struct("AppError", 3)?;
        st.serialize_field("kind", kind)?;
        st.serialize_field("message", &self.localised())?;
        match self {
            AppError::SimParams(errs) => {
                let translated: Vec<crate::sim::ParamError> = errs
                    .iter()
                    .map(|e| crate::sim::ParamError {
                        field: e.field,
                        message: crate::i18n::translate(&e.message),
                    })
                    .collect();
                st.serialize_field("details", &translated)?
            }
            _ => st.serialize_field("details", &Option::<()>::None)?,
        }
        st.end()
    }
}

impl AppError {
    /// The message as the person reading it should see it.
    ///
    /// Written in English where the error is raised and translated here, on
    /// the way out, so the code that knows what went wrong does not also have
    /// to know who is reading.
    pub fn localised(&self) -> String {
        use crate::i18n::{pick, translate};
        match self {
            AppError::Db(e) => format!("{}: {e}", pick("database error", "error de base de datos")),
            AppError::Migration(e) => {
                format!("{}: {e}", pick("migration error", "error de migración"))
            }
            AppError::Keystore(m) => format!(
                "{}: {}",
                pick("keystore error", "error del almacén de claves"),
                translate(m)
            ),
            AppError::Locked => pick(
                "the database is locked; unlock it before using this command",
                "la base de datos está bloqueada; desbloquéala antes de usar esta acción",
            )
            .to_string(),
            AppError::Invalid(m) => format!(
                "{}: {}",
                pick("invalid input", "dato no válido"),
                translate(m)
            ),
            AppError::SimParams(_) => pick(
                "simulation parameters are invalid",
                "los parámetros de la simulación no son válidos",
            )
            .to_string(),
            AppError::Network(m) => format!(
                "{}: {}",
                pick("network request failed", "falló la conexión"),
                translate(m)
            ),
            AppError::Other(m) => translate(m),
        }
    }
}
