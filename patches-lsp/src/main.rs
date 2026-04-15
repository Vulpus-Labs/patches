mod analysis;
mod ast;
mod ast_builder;
mod completions;
mod expansion;
mod hover;
mod inlay;
mod lsp_util;
mod peek;
mod navigation;
mod parser;
mod server;
mod shape_render;
mod signal_graph;
mod workspace;

use server::PatchesLanguageServer;
use tower_lsp::{LspService, Server};

#[tokio::main]
async fn main() {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = LspService::build(PatchesLanguageServer::new)
        .custom_method("patches/renderSvg", PatchesLanguageServer::render_svg)
        .finish();

    Server::new(stdin, stdout, socket).serve(service).await;
}
