//! Persistent storage for agent provider configuration.

use rusqlite::params;
use uuid::Uuid;

use crate::concurrency::{AgentId, AgentLlmConfig};
use crate::error::ApiError;

#[derive(Debug, Clone)]
pub struct PersistedAgentConfig {
    pub id: AgentId,
    pub name: Option<String>,
    pub llm: AgentLlmConfig,
}

#[derive(Debug, Clone)]
pub struct AgentConfigStore {
    db_path: String,
}

impl AgentConfigStore {
    pub fn new(db_path: &str) -> Result<Self, ApiError> {
        let store = Self {
            db_path: db_path.to_string(),
        };
        // Ensure DB exists and migrations are applied.
        let _conn = store.open_conn()?;
        Ok(store)
    }

    pub fn in_memory() -> Result<Self, ApiError> {
        let temp_path = std::env::temp_dir()
            .join(format!("lmlang_agent_cfg_{}.db", Uuid::new_v4()))
            .to_string_lossy()
            .to_string();
        Self::new(&temp_path)
    }

    pub fn list(&self) -> Result<Vec<PersistedAgentConfig>, ApiError> {
        let conn = self.open_conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT agent_id, name, provider, model, api_base_url, api_key, system_prompt
             FROM agent_configs
             ORDER BY updated_at DESC, agent_id ASC",
            )
            .map_err(db_err)?;

        let rows = stmt
            .query_map([], |row| {
                let raw_id: String = row.get(0)?;
                let parsed_uuid = Uuid::parse_str(&raw_id).ok();
                let id = parsed_uuid.map(AgentId);
                Ok((
                    id,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, Option<String>>(6)?,
                ))
            })
            .map_err(db_err)?;

        let mut out = Vec::new();
        for row in rows {
            let (id, name, provider, model, api_base_url, api_key, system_prompt) =
                row.map_err(db_err)?;
            let Some(id) = id else {
                continue;
            };
            out.push(PersistedAgentConfig {
                id,
                name,
                llm: AgentLlmConfig {
                    provider,
                    model,
                    api_base_url,
                    api_key,
                    system_prompt,
                }
                .normalize(),
            });
        }
        Ok(out)
    }

    pub fn upsert(
        &self,
        agent_id: AgentId,
        name: Option<String>,
        llm: &AgentLlmConfig,
    ) -> Result<(), ApiError> {
        let conn = self.open_conn()?;
        let llm = llm.clone().normalize();

        conn.execute(
            "INSERT INTO agent_configs (
                 agent_id, name, provider, model, api_base_url, api_key, system_prompt, created_at, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
             ON CONFLICT(agent_id) DO UPDATE SET
                 name = excluded.name,
                 provider = excluded.provider,
                 model = excluded.model,
                 api_base_url = excluded.api_base_url,
                 api_key = excluded.api_key,
                 system_prompt = excluded.system_prompt,
                 updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
            params![
                agent_id.0.to_string(),
                name,
                llm.provider,
                llm.model,
                llm.api_base_url,
                llm.api_key,
                llm.system_prompt,
            ],
        )
        .map_err(db_err)?;

        Ok(())
    }

    pub fn delete(&self, agent_id: AgentId) -> Result<(), ApiError> {
        let conn = self.open_conn()?;
        conn.execute(
            "DELETE FROM agent_configs WHERE agent_id = ?1",
            params![agent_id.0.to_string()],
        )
        .map_err(db_err)?;
        Ok(())
    }

    fn open_conn(&self) -> Result<rusqlite::Connection, ApiError> {
        lmlang_storage::schema::open_database(&self.db_path).map_err(|err| {
            ApiError::InternalError(format!("failed to open agent config DB: {}", err))
        })
    }
}

fn db_err(err: rusqlite::Error) -> ApiError {
    ApiError::InternalError(format!("agent config store query failed: {}", err))
}
