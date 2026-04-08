mod analysis;
mod ast;
mod ast_builder;
mod completions;
mod hover;
mod lsp_util;
mod navigation;
mod parser;
mod server;

use server::PatchesLanguageServer;
use tower_lsp::{LspService, Server};

#[tokio::main]
async fn main() {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = LspService::new(PatchesLanguageServer::new);

    Server::new(stdin, stdout, socket).serve(service).await;
}
