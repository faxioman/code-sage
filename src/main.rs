use rmcp::{
    handler::server::router::tool::ToolRouter,
    model::*,
    tool, tool_handler,
    transport::stdio,
    ServiceExt,
};
use schemars::JsonSchema;
use serde::Deserialize;
use std::sync::Arc;
use code_sage::embeddings::EmbeddingProvider;

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct IndexCodebaseParams {
    #[schemars(description = "Absolute path to the codebase directory to index")]
    path: String,
    #[schemars(description = "Force re-indexing even if already indexed")]
    #[serde(default)]
    force: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct SearchCodeParams {
    #[schemars(description = "Absolute path to the indexed codebase directory")]
    path: String,
    #[schemars(description = "Natural language search query")]
    query: String,
    #[schemars(description = "Maximum number of results to return")]
    #[serde(default = "default_limit")]
    limit: usize,
}

fn default_limit() -> usize {
    10
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct ClearIndexParams {
    #[schemars(description = "Absolute path to the codebase directory to clear")]
    path: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct GetIndexingStatusParams {
    #[schemars(description = "Absolute path to the codebase directory")]
    path: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .init();

    tracing::info!("Starting Code Sage MCP Server");

    let config = code_sage::Config::from_env()?;
    tracing::info!("Configuration loaded");

    let snapshot_path = config.storage.data_dir.join("snapshot.json");
    let snapshot = code_sage::snapshot::SnapshotManager::new(snapshot_path)?;
    tracing::info!("Snapshot manager loaded");

    let embedding: Arc<dyn code_sage::embeddings::EmbeddingProvider> = match config.embedding.provider {
        code_sage::config::EmbeddingProvider::OpenAI => {
            let api_key = config.embedding.api_key.clone()
                .ok_or_else(|| code_sage::Error::Config("Missing OPENAI_API_KEY".to_string()))?;
            let mut openai = code_sage::embeddings::OpenAIEmbedding::new(
                api_key,
                Some(config.embedding.model.clone()),
                config.embedding.base_url.clone(),
            );
            
            if let Err(e) = openai.detect_dimension().await {
                tracing::warn!("Failed to detect dimension: {}. Model may not be available.", e);
                return Err(code_sage::Error::Config(
                    format!("Failed to initialize OpenAI with model '{}'. Please ensure the API is accessible.",
                        config.embedding.model)
                ).into());
            }
            
            tracing::info!("OpenAI initialized with model '{}' (dimension: {})",
                config.embedding.model, openai.dimension());
            
            Arc::new(openai)
        }
        code_sage::config::EmbeddingProvider::Ollama => {
            let mut ollama = code_sage::embeddings::OllamaEmbedding::new(
                Some(config.embedding.base_url.clone().unwrap_or_else(|| "http://localhost:11434".to_string())),
                Some(config.embedding.model.clone()),
            );
            
            if let Err(e) = ollama.initialize().await {
                tracing::warn!("Failed to initialize Ollama: {}. Model may not be available.", e);
                return Err(code_sage::Error::Config(
                    format!("Failed to initialize Ollama with model '{}'. Please ensure Ollama is running and the model is pulled.",
                        config.embedding.model)
                ).into());
            }
            
            tracing::info!("Ollama initialized with model '{}'", config.embedding.model);
            
            Arc::new(ollama)
        }
    };
    tracing::info!("Embedding provider initialized: {}", embedding.provider_name());

    let handlers = code_sage::handlers::ToolHandlers::new(
        config.clone(),
        snapshot,
        embedding,
    );
    tracing::info!("Tool handlers initialized");

    let server = EmbeddingsContextServer::new(Arc::new(handlers));

    tracing::info!("Server initialized, starting stdio transport");
    
    let service = server.serve(stdio()).await?;
    service.waiting().await?;

    Ok(())
}

struct EmbeddingsContextServer {
    handlers: Arc<code_sage::handlers::ToolHandlers>,
    tool_router: ToolRouter<Self>,
}

impl EmbeddingsContextServer {
    fn new(handlers: Arc<code_sage::handlers::ToolHandlers>) -> Self {
        Self {
            handlers,
            tool_router: Self::tool_router(),
        }
    }
}

#[rmcp::tool_router]
impl EmbeddingsContextServer {
    #[tool(
        name = "analyze_code",
        description = "Create a searchable index of your code by analyzing functions, classes, and methods. This enables smart code search with natural language queries."
    )]
    async fn index_codebase(
        &self,
        params: rmcp::handler::server::wrapper::Parameters<IndexCodebaseParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let params = params.0;
        let args = code_sage::handlers::IndexCodebaseArgs {
            path: params.path,
            force: params.force,
            splitter: "ast".to_string(),
            custom_extensions: vec![],
            ignore_patterns: vec![],
        };

        match self.handlers.handle_index_codebase(args).await {
            Ok(json_response) => Ok(CallToolResult::success(vec![Content::text(json_response)])),
            Err(e) => Ok(CallToolResult::success(vec![Content::text(
                serde_json::json!({"error": format!("Indexing failed: {}", e)}).to_string()
            )])),
        }
    }

    #[tool(
        name = "find_code",
        description = "Find code using natural language questions. Combines keyword search with AI understanding to locate relevant functions, classes, and code patterns."
    )]
    async fn search_code(
        &self,
        params: rmcp::handler::server::wrapper::Parameters<SearchCodeParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let params = params.0;
        let args = code_sage::handlers::SearchCodeArgs {
            path: params.path,
            query: params.query,
            limit: params.limit,
            extension_filter: vec![],
        };
        
        match self.handlers.handle_search_code(args).await {
            Ok(json_response) => Ok(CallToolResult::success(vec![Content::text(json_response)])),
            Err(e) => Ok(CallToolResult::success(vec![Content::text(
                serde_json::json!({"error": format!("Search failed: {}", e)}).to_string()
            )])),
        }
    }

    #[tool(
        name = "delete_index",
        description = "Delete the search index for a codebase to free up space or start fresh. Removes all stored code analysis."
    )]
    async fn clear_index(
        &self,
        params: rmcp::handler::server::wrapper::Parameters<ClearIndexParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let params = params.0;
        let args = code_sage::handlers::ClearIndexArgs {
            path: params.path,
        };
        
        match self.handlers.handle_clear_index(args).await {
            Ok(json_response) => Ok(CallToolResult::success(vec![Content::text(json_response)])),
            Err(e) => Ok(CallToolResult::success(vec![Content::text(
                serde_json::json!({"error": format!("Clear failed: {}", e)}).to_string()
            )])),
        }
    }

    #[tool(
        name = "check_status",
        description = "Check if code analysis is complete, in progress, or failed. Shows percentage done and number of files processed."
    )]
    async fn get_indexing_status(
        &self,
        params: rmcp::handler::server::wrapper::Parameters<GetIndexingStatusParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let params = params.0;
        let args = code_sage::handlers::GetIndexingStatusArgs {
            path: params.path,
        };
        
        match self.handlers.handle_get_indexing_status(args).await {
            Ok(json_response) => Ok(CallToolResult::success(vec![Content::text(json_response)])),
            Err(e) => Ok(CallToolResult::success(vec![Content::text(
                serde_json::json!({"error": format!("Status check failed: {}", e)}).to_string()
            )])),
        }
    }
}

#[tool_handler]
impl rmcp::ServerHandler for EmbeddingsContextServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: ProtocolVersion::V_2024_11_05,
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            server_info: Implementation::from_build_env(),
            instructions: Some(
                "Semantic code search server. Use index_codebase to index, \
                 then search_code to find relevant code with natural language.".to_string()
            ),
        }
    }
}
