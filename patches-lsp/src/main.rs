mod analysis;
mod ast;
mod ast_builder;
mod completions;
mod expansion;
mod hover;
mod inlay;
mod lsp_util;
mod manifest_source;
mod peek;
mod navigation;
mod parser;
mod server;
mod shape_render;
#[cfg(test)]
mod syntax_corpus;
mod tree_nav;
mod workspace;

use server::PatchesLanguageServer;
use tower_lsp::{LspService, Server};

#[tokio::main]
async fn main() {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = LspService::build(PatchesLanguageServer::new)
        .custom_method("patches/renderSvg", PatchesLanguageServer::render_svg)
        .custom_method("patches/graphJson", PatchesLanguageServer::graph_json)
        .custom_method("patches/rescanModules", PatchesLanguageServer::rescan_modules)
        .finish();

    Server::new(stdin, stdout, socket).serve(service).await;
}
