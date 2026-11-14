//! Language server.
//!
//! Speaks the Language Server Protocol over standard input and output:
//! it publishes diagnostics every time a document changes and offers the
//! fixes attached to them as code actions.

// `lsp_types::Uri` caches parse data in a `Cell`, which trips this lint, but
// the protocol types themselves use it as a map key (`WorkspaceEdit::changes`).
#![allow(clippy::mutable_key_type)]

use std::collections::HashMap;
use std::error::Error;
use std::str::FromStr;

use lsp_server::{Connection, ErrorCode, Message, Notification, Request, Response};
use lsp_types::{
    CodeAction, CodeActionKind, CodeActionOrCommand, CodeActionParams,
    CodeActionProviderCapability, CodeDescription, CompletionItem, CompletionItemKind,
    CompletionOptions, CompletionParams, DiagnosticRelatedInformation, DiagnosticSeverity,
    DiagnosticTag, DidChangeConfigurationParams, DidChangeTextDocumentParams,
    DidCloseTextDocumentParams, DidOpenTextDocumentParams, DocumentSymbol, DocumentSymbolParams,
    GotoDefinitionParams, Hover, HoverContents, HoverParams, HoverProviderCapability,
    InitializeParams, InlayHint, InlayHintLabel, InlayHintParams, Location, MarkupContent,
    MarkupKind, NumberOrString, OneOf, Position, PublishDiagnosticsParams, Range, ReferenceParams,
    RenameParams, ServerCapabilities, SymbolKind, TextDocumentPositionParams,
    TextDocumentSyncCapability, TextDocumentSyncKind, TextEdit, Uri, WorkspaceEdit,
};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::diagnostic::{Diagnostic, Edit, Severity};
use crate::fix;
use crate::ide::{CompletionKind, Document, SymbolClass};
use crate::span::Span;
use crate::{Config, check};

type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync>>;

/// Where each lint is documented; the lint name is the anchor.
const DOCS: &str = "https://github.com/pwned841/asm-check/blob/main/docs/lints.md";

/// Lints about code that can simply be deleted. Editors fade such code out.
const UNNECESSARY: &[&str] = &[
    "dead-store",
    "unreachable-code",
    "unused-label",
    "unused-comparison",
    "self-move",
    "redundant-jump",
    "noop-arithmetic",
];

/// Runs the server on standard input and output until the client exits.
pub fn run() -> Result<()> {
    let (connection, io_threads) = Connection::stdio();
    serve(connection)?;
    io_threads.join()?;
    Ok(())
}

/// Runs the server on an established connection.
pub fn serve(connection: Connection) -> Result<()> {
    let capabilities = ServerCapabilities {
        text_document_sync: Some(TextDocumentSyncCapability::Kind(TextDocumentSyncKind::FULL)),
        code_action_provider: Some(CodeActionProviderCapability::Simple(true)),
        hover_provider: Some(HoverProviderCapability::Simple(true)),
        definition_provider: Some(OneOf::Left(true)),
        references_provider: Some(OneOf::Left(true)),
        document_symbol_provider: Some(OneOf::Left(true)),
        rename_provider: Some(OneOf::Left(true)),
        inlay_hint_provider: Some(OneOf::Left(true)),
        completion_provider: Some(CompletionOptions {
            trigger_characters: Some(vec![".".to_string()]),
            ..CompletionOptions::default()
        }),
        ..ServerCapabilities::default()
    };
    let params = connection.initialize(serde_json::to_value(capabilities)?)?;
    let params: InitializeParams = serde_json::from_value(params)?;
    let mut server = Server {
        config: settings(params.initialization_options.as_ref()),
        documents: HashMap::new(),
    };

    for message in &connection.receiver {
        match message {
            Message::Request(request) => {
                if connection.handle_shutdown(&request)? {
                    break;
                }
                connection.sender.send(Message::Response(server.request(request)))?;
            }
            Message::Notification(notification) => {
                for reply in server.notification(notification) {
                    connection.sender.send(Message::Notification(reply))?;
                }
            }
            Message::Response(_) => {}
        }
    }
    Ok(())
}

/// Reads `{"pedantic": bool, "allow": [lint, ...]}`, either at the top level
/// or under an `asm-check` key. Anything missing keeps its default.
fn settings(value: Option<&Value>) -> Config {
    let Some(value) = value else { return Config::default() };
    let value = value.get("asm-check").unwrap_or(value);
    Config {
        pedantic: value.get("pedantic").and_then(Value::as_bool).unwrap_or(false),
        allow: value
            .get("allow")
            .and_then(Value::as_array)
            .map(|names| names.iter().filter_map(Value::as_str).map(str::to_string).collect())
            .unwrap_or_default(),
    }
}

struct Server {
    config: Config,
    /// Open documents and their current text.
    documents: HashMap<Uri, String>,
}

impl Server {
    fn request(&self, request: Request) -> Response {
        let Request { id, method, params } = request;
        match method.as_str() {
            "textDocument/codeAction" => answer(id, params, |p: CodeActionParams| {
                let uri = &p.text_document.uri;
                self.documents
                    .get(uri)
                    .map(|text| code_actions(uri, text, &self.config, p.range))
                    .unwrap_or_default()
            }),
            "textDocument/hover" => answer(id, params, |p: HoverParams| {
                let (uri, document, lines, line, column) =
                    self.locate(&p.text_document_position_params)?;
                let (span, markdown) = document.hover(line, column)?;
                let _ = uri;
                Some(Hover {
                    contents: HoverContents::Markup(MarkupContent {
                        kind: MarkupKind::Markdown,
                        value: markdown,
                    }),
                    range: Some(range(&lines, span)),
                })
            }),
            "textDocument/definition" => answer(id, params, |p: GotoDefinitionParams| {
                let (uri, document, lines, line, column) =
                    self.locate(&p.text_document_position_params)?;
                let span = document.definition(line, column)?;
                Some(Location::new(uri, range(&lines, span)))
            }),
            "textDocument/references" => answer(id, params, |p: ReferenceParams| {
                let Some((uri, document, lines, line, column)) =
                    self.locate(&p.text_document_position)
                else {
                    return Vec::new();
                };
                document
                    .references(line, column)
                    .into_iter()
                    .map(|span| Location::new(uri.clone(), range(&lines, span)))
                    .collect()
            }),
            "textDocument/rename" => {
                let edit = serde_json::from_value::<RenameParams>(params).ok().and_then(|p| {
                    let (uri, document, lines, line, column) =
                        self.locate(&p.text_document_position)?;
                    let edits = document
                        .rename(line, column, &p.new_name)?
                        .into_iter()
                        .map(|(span, text)| TextEdit::new(range(&lines, span), text))
                        .collect();
                    Some(WorkspaceEdit {
                        changes: Some(HashMap::from([(uri, edits)])),
                        ..WorkspaceEdit::default()
                    })
                });
                match edit {
                    Some(edit) => Response::new_ok(id, edit),
                    None => Response::new_err(
                        id,
                        ErrorCode::InvalidParams as i32,
                        "only labels can be renamed, to a valid label name".to_string(),
                    ),
                }
            }
            "textDocument/documentSymbol" => answer(id, params, |p: DocumentSymbolParams| {
                let Some(text) = self.documents.get(&p.text_document.uri) else {
                    return Vec::new();
                };
                let lines: Vec<&str> = text.lines().collect();
                Document::new(text)
                    .outline()
                    .into_iter()
                    .map(|entry| {
                        let range = range(&lines, entry.span);
                        #[allow(deprecated)] // the `deprecated` field has no default
                        DocumentSymbol {
                            name: entry.name,
                            detail: None,
                            kind: match entry.class {
                                SymbolClass::Function => SymbolKind::FUNCTION,
                                SymbolClass::Label => SymbolKind::KEY,
                                SymbolClass::Data => SymbolKind::VARIABLE,
                                SymbolClass::Constant => SymbolKind::CONSTANT,
                            },
                            tags: None,
                            deprecated: None,
                            range,
                            selection_range: range,
                            children: None,
                        }
                    })
                    .collect()
            }),
            "textDocument/inlayHint" => answer(id, params, |p: InlayHintParams| {
                let Some(text) = self.documents.get(&p.text_document.uri) else {
                    return Vec::new();
                };
                let lines: Vec<&str> = text.lines().collect();
                Document::new(text)
                    .inlay_hints()
                    .into_iter()
                    .filter(|(line, ..)| {
                        (p.range.start.line..=p.range.end.line).contains(&(*line as u32))
                    })
                    .map(|(line, column, label)| InlayHint {
                        position: position(&lines, line, column),
                        label: InlayHintLabel::String(label),
                        kind: None,
                        text_edits: None,
                        tooltip: None,
                        padding_left: Some(true),
                        padding_right: None,
                        data: None,
                    })
                    .collect()
            }),
            "textDocument/completion" => answer(id, params, |p: CompletionParams| {
                let Some((_, document, _, line, column)) = self.locate(&p.text_document_position)
                else {
                    return Vec::new();
                };
                document
                    .completions(line, column)
                    .into_iter()
                    .map(|item| CompletionItem {
                        label: item.label,
                        kind: Some(match item.kind {
                            CompletionKind::Instruction => CompletionItemKind::FUNCTION,
                            CompletionKind::Register => CompletionItemKind::VARIABLE,
                            CompletionKind::Label => CompletionItemKind::REFERENCE,
                            CompletionKind::Keyword => CompletionItemKind::KEYWORD,
                        }),
                        detail: (!item.detail.is_empty()).then_some(item.detail),
                        ..CompletionItem::default()
                    })
                    .collect()
            }),
            method => Response::new_err(
                id,
                ErrorCode::MethodNotFound as i32,
                format!("unsupported request `{method}`"),
            ),
        }
    }

    /// Resolves a cursor position: the document, its lines, and the cursor
    /// as a line and a byte column.
    fn locate(
        &self,
        at: &TextDocumentPositionParams,
    ) -> Option<(Uri, Document, Vec<&str>, usize, usize)> {
        let uri = at.text_document.uri.clone();
        let text = self.documents.get(&uri)?;
        let lines: Vec<&str> = text.lines().collect();
        let line = at.position.line as usize;
        let column = byte_column(lines.get(line).copied().unwrap_or(""), at.position.character);
        Some((uri, Document::new(text), lines, line, column))
    }

    /// Handles a notification and returns the notifications to send back.
    fn notification(&mut self, notification: Notification) -> Vec<Notification> {
        let Notification { method, params } = notification;
        match method.as_str() {
            "textDocument/didOpen" => {
                let Ok(params) = serde_json::from_value::<DidOpenTextDocumentParams>(params) else {
                    return Vec::new();
                };
                let uri = params.text_document.uri;
                self.documents.insert(uri.clone(), params.text_document.text);
                vec![self.publish(&uri)]
            }
            "textDocument/didChange" => {
                let Ok(params) = serde_json::from_value::<DidChangeTextDocumentParams>(params)
                else {
                    return Vec::new();
                };
                let uri = params.text_document.uri;
                // Full sync: the last change carries the whole document.
                let Some(change) = params.content_changes.into_iter().next_back() else {
                    return Vec::new();
                };
                self.documents.insert(uri.clone(), change.text);
                vec![self.publish(&uri)]
            }
            "textDocument/didClose" => {
                let Ok(params) = serde_json::from_value::<DidCloseTextDocumentParams>(params)
                else {
                    return Vec::new();
                };
                let uri = params.text_document.uri;
                self.documents.remove(&uri);
                vec![self.publish(&uri)]
            }
            "workspace/didChangeConfiguration" => {
                let Ok(params) = serde_json::from_value::<DidChangeConfigurationParams>(params)
                else {
                    return Vec::new();
                };
                self.config = settings(Some(&params.settings));
                self.documents.keys().map(|uri| self.publish(uri)).collect()
            }
            _ => Vec::new(),
        }
    }

    /// Diagnostics for a document; an empty list once it is closed.
    fn publish(&self, uri: &Uri) -> Notification {
        let diagnostics = self
            .documents
            .get(uri)
            .map(|text| {
                let lines: Vec<&str> = text.lines().collect();
                check(text, &self.config).iter().map(|d| to_lsp(uri, &lines, d)).collect()
            })
            .unwrap_or_default();
        let params = PublishDiagnosticsParams { uri: uri.clone(), diagnostics, version: None };
        Notification::new("textDocument/publishDiagnostics".to_string(), params)
    }
}

/// Decodes the parameters, runs the handler and wraps its result.
fn answer<P: DeserializeOwned, R: Serialize>(
    id: lsp_server::RequestId,
    params: Value,
    handler: impl FnOnce(P) -> R,
) -> Response {
    match serde_json::from_value(params) {
        Ok(params) => Response::new_ok(id, handler(params)),
        Err(error) => Response::new_err(id, ErrorCode::InvalidParams as i32, error.to_string()),
    }
}

/// The byte offset of a UTF-16 column in a line.
fn byte_column(line: &str, character: u32) -> usize {
    let mut units = 0;
    for (offset, c) in line.char_indices() {
        if units >= character {
            return offset;
        }
        units += c.len_utf16() as u32;
    }
    line.len()
}

/// LSP positions count UTF-16 code units; spans count bytes.
fn position(lines: &[&str], line: usize, byte: usize) -> Position {
    let text = lines.get(line).copied().unwrap_or("");
    let prefix = text.get(..byte).unwrap_or(text);
    Position::new(line as u32, prefix.encode_utf16().count() as u32)
}

fn range(lines: &[&str], span: Span) -> Range {
    Range::new(position(lines, span.line, span.start), position(lines, span.line, span.end))
}

fn to_lsp(uri: &Uri, lines: &[&str], diagnostic: &Diagnostic) -> lsp_types::Diagnostic {
    let name = diagnostic.lint.name;
    // The editor shows plain text: the headline, then the fix on one line.
    let mut message = diagnostic.message.clone();
    match (&diagnostic.fix, &diagnostic.help) {
        (Some(fix), _) => {
            let summary = fix::summary(fix, lines);
            if !summary.is_empty() {
                message.push('\n');
                message.push_str(&summary);
            }
        }
        (None, Some(help)) => message.push_str(&format!("\nhelp: {help}")),
        (None, None) => {}
    }
    let related: Vec<DiagnosticRelatedInformation> = diagnostic
        .notes
        .iter()
        .filter_map(|note| {
            Some(DiagnosticRelatedInformation {
                location: Location::new(uri.clone(), range(lines, note.span?)),
                message: note.message.clone(),
            })
        })
        .collect();

    lsp_types::Diagnostic {
        range: range(lines, diagnostic.span),
        severity: Some(match diagnostic.severity {
            Severity::Error => DiagnosticSeverity::ERROR,
            Severity::Warning => DiagnosticSeverity::WARNING,
            Severity::Hint => DiagnosticSeverity::HINT,
        }),
        code: Some(NumberOrString::String(name.to_string())),
        code_description: Uri::from_str(&format!("{DOCS}#{name}"))
            .ok()
            .map(|href| CodeDescription { href }),
        source: Some("asm-check".to_string()),
        message,
        related_information: (!related.is_empty()).then_some(related),
        tags: UNNECESSARY.contains(&name).then(|| vec![DiagnosticTag::UNNECESSARY]),
        data: None,
    }
}

/// Quick fixes for the diagnostics on the requested lines: the fix attached
/// to each one, and a comment that silences it.
fn code_actions(
    uri: &Uri,
    text: &str,
    config: &Config,
    requested: Range,
) -> Vec<CodeActionOrCommand> {
    let lines: Vec<&str> = text.lines().collect();
    let mut actions = Vec::new();
    let action = |title: String, diagnostic: &Diagnostic, edits: Vec<TextEdit>, preferred: bool| {
        CodeActionOrCommand::CodeAction(CodeAction {
            title,
            kind: Some(CodeActionKind::QUICKFIX),
            diagnostics: Some(vec![to_lsp(uri, &lines, diagnostic)]),
            edit: Some(WorkspaceEdit {
                changes: Some(HashMap::from([(uri.clone(), edits)])),
                ..WorkspaceEdit::default()
            }),
            is_preferred: Some(preferred),
            ..CodeAction::default()
        })
    };

    for diagnostic in check(text, config) {
        let line = diagnostic.span.line;
        if (line as u32) < requested.start.line || (line as u32) > requested.end.line {
            continue;
        }
        if let Some(fix) = &diagnostic.fix {
            let edits = fix.edits.iter().map(|edit| text_edit(&lines, edit)).collect();
            actions.push(action(capitalise(&fix.title), &diagnostic, edits, fix.safe));
        }
        let name = diagnostic.lint.name;
        let end = position(&lines, line, lines.get(line).map_or(0, |text| text.trim_end().len()));
        let silence = TextEdit::new(Range::new(end, end), format!(" ; asm-check: allow({name})"));
        actions.push(action(
            format!("Silence `{name}` on this line"),
            &diagnostic,
            vec![silence],
            false,
        ));
    }
    actions
}

/// Translates an edit, giving new lines the indentation of their neighbour.
fn text_edit(lines: &[&str], edit: &Edit) -> TextEdit {
    let line_text = |line: usize| lines.get(line).copied().unwrap_or("");
    let indent = |line: usize| {
        let text = line_text(line);
        text[..text.len() - text.trim_start().len()].to_string()
    };
    let block = |line: usize, text: &str| -> Vec<String> {
        text.lines().map(|row| format!("{}{row}", indent(line))).collect()
    };
    match edit {
        Edit::Replace { span, text } => {
            let mut rows = text.lines();
            let mut new_text = rows.next().unwrap_or("").to_string();
            for row in rows {
                new_text.push_str(&format!("\n{}{row}", indent(span.line)));
            }
            TextEdit::new(range(lines, *span), new_text)
        }
        Edit::DeleteLine(line) => TextEdit::new(
            Range::new(Position::new(*line as u32, 0), Position::new(*line as u32 + 1, 0)),
            String::new(),
        ),
        Edit::InsertBefore { line, text } => {
            let at = Position::new(*line as u32, 0);
            TextEdit::new(Range::new(at, at), format!("{}\n", block(*line, text).join("\n")))
        }
        Edit::InsertAfter { line, text } => {
            let at = position(lines, *line, line_text(*line).len());
            TextEdit::new(Range::new(at, at), format!("\n{}", block(*line, text).join("\n")))
        }
    }
}

fn capitalise(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use std::thread;

    use serde_json::json;

    use super::*;

    fn uri() -> Uri {
        Uri::from_str("file:///tmp/test.s").unwrap()
    }

    #[test]
    fn byte_columns() {
        let line = "msg db 'é𝄞' ; x";
        assert_eq!(byte_column(line, 8), 8);
        assert_eq!(byte_column(line, 9), 10);
        assert_eq!(byte_column(line, 11), 14);
        assert_eq!(byte_column(line, 999), line.len());
    }

    #[test]
    fn positions_are_utf16() {
        // 'é' is 2 bytes but 1 UTF-16 unit; '𝄞' is 4 bytes and 2 units.
        let lines = ["msg db 'é𝄞' ; x"];
        assert_eq!(position(&lines, 0, 8), Position::new(0, 8));
        assert_eq!(position(&lines, 0, 10), Position::new(0, 9));
        assert_eq!(position(&lines, 0, 14), Position::new(0, 11));
        assert_eq!(position(&lines, 5, 3), Position::new(5, 0));
    }

    #[test]
    fn diagnostics_carry_code_help_and_related_information() {
        let text = "f:\n    mov rax, 1\n    mov rax, 2\n    ret\n";
        let lines: Vec<&str> = text.lines().collect();
        let found = check(text, &Config::default());
        let dead = found.iter().find(|d| d.lint.name == "dead-store").expect("a dead store");
        let lsp = to_lsp(&uri(), &lines, dead);
        assert_eq!(lsp.range, Range::new(Position::new(1, 4), Position::new(1, 14)));
        assert_eq!(lsp.severity, Some(DiagnosticSeverity::WARNING));
        assert_eq!(lsp.code, Some(NumberOrString::String("dead-store".into())));
        assert_eq!(lsp.message.lines().last(), Some("fix: remove mov rax, 1"));
        assert_eq!(lsp.related_information.unwrap()[0].location.range.start.line, 2);
        assert_eq!(lsp.tags, Some(vec![DiagnosticTag::UNNECESSARY]));
        assert!(lsp.code_description.unwrap().href.as_str().ends_with("#dead-store"));
    }

    #[test]
    fn code_actions_fix_and_silence() {
        let text = "f:\n    mov rax, 0\n    ret\n";
        let line = Range::new(Position::new(1, 0), Position::new(1, 0));
        let actions = code_actions(&uri(), text, &Config::default(), line);
        let edits: Vec<(String, TextEdit)> = actions
            .into_iter()
            .map(|action| match action {
                CodeActionOrCommand::CodeAction(action) => {
                    let mut changes = action.edit.unwrap().changes.unwrap();
                    (action.title, changes.remove(&uri()).unwrap().remove(0))
                }
                CodeActionOrCommand::Command(_) => unreachable!(),
            })
            .collect();
        assert_eq!(edits[0].0, "Use `xor eax, eax`");
        assert_eq!(edits[0].1.new_text, "xor eax, eax");
        assert_eq!(edits[0].1.range, Range::new(Position::new(1, 4), Position::new(1, 14)));
        assert_eq!(edits[1].1.new_text, " ; asm-check: allow(mov-zero)");
        assert_eq!(edits[1].1.range.start, Position::new(1, 14));

        let elsewhere = Range::new(Position::new(2, 0), Position::new(2, 3));
        assert!(code_actions(&uri(), text, &Config::default(), elsewhere).is_empty());
    }

    #[test]
    fn reads_settings() {
        let config =
            settings(Some(&json!({"asm-check": {"pedantic": true, "allow": ["mov-zero"]}})));
        assert!(config.pedantic && config.allow.contains("mov-zero"));
        let config = settings(Some(&json!({"allow": ["cmp-zero"]})));
        assert!(!config.pedantic && config.allow.contains("cmp-zero"));
        assert!(!settings(Some(&json!(null))).pedantic);
    }

    /// Drives a whole session through an in-memory connection.
    #[test]
    fn session() {
        let (server, client) = Connection::memory();
        let handle = thread::spawn(move || serve(server).unwrap());
        let send = |message: Message| client.sender.send(message).unwrap();
        let request = |id: i32, method: &str, params: Value| {
            send(Message::Request(Request::new(id.into(), method.to_string(), params)));
        };
        let notify = |method: &str, params: Value| {
            send(Message::Notification(Notification::new(method.to_string(), params)));
        };
        let diagnostics = || match client.receiver.recv().unwrap() {
            Message::Notification(n) if n.method == "textDocument/publishDiagnostics" => {
                serde_json::from_value::<PublishDiagnosticsParams>(n.params).unwrap().diagnostics
            }
            other => panic!("expected diagnostics, got {other:?}"),
        };
        let response = || match client.receiver.recv().unwrap() {
            Message::Response(response) => response,
            other => panic!("expected a response, got {other:?}"),
        };

        request(
            1,
            "initialize",
            json!({"capabilities": {}, "initializationOptions": {"allow": ["unused-label"]}}),
        );
        let capabilities = response().response_result.unwrap();
        assert_eq!(capabilities["capabilities"]["textDocumentSync"], json!(1));
        notify("initialized", json!({}));

        let document = json!({"uri": "file:///tmp/test.s", "languageId": "asm", "version": 1,
            "text": "f:\n    mov rcx, 4\n    syscall\n    dec rcx\n    ret\n"});
        notify("textDocument/didOpen", json!({"textDocument": document}));
        let found = diagnostics();
        let codes: Vec<_> = found.iter().map(|d| d.code.clone().unwrap()).collect();
        assert_eq!(
            codes,
            [
                NumberOrString::String("dead-store".into()),
                NumberOrString::String("clobbered-register".into())
            ]
        );

        notify(
            "textDocument/didChange",
            json!({"textDocument": {"uri": "file:///tmp/test.s", "version": 2},
                "contentChanges": [{"text": "f:\n    ret\n"}]}),
        );
        assert!(diagnostics().is_empty());

        notify("workspace/didChangeConfiguration", json!({"settings": {"allow": []}}));
        assert_eq!(diagnostics().len(), 1); // unused-label is back

        let at = |line: u32, character: u32| {
            json!({"textDocument": {"uri": "file:///tmp/test.s"},
                "position": {"line": line, "character": character}})
        };
        notify(
            "textDocument/didChange",
            json!({"textDocument": {"uri": "file:///tmp/test.s", "version": 3},
                "contentChanges": [{"text": "f:\n    mov eax, 60\n    syscall\n    jmp f\n"}]}),
        );
        diagnostics();
        request(10, "textDocument/hover", at(1, 5));
        let hover = response().response_result.unwrap();
        assert!(hover["contents"]["value"].as_str().unwrap().starts_with("**mov**"));
        request(11, "textDocument/definition", at(3, 8));
        assert_eq!(response().response_result.unwrap()["range"]["start"]["line"], json!(0));
        request(12, "textDocument/references", {
            let mut params = at(3, 8);
            params["context"] = json!({"includeDeclaration": true});
            params
        });
        assert_eq!(response().response_result.unwrap().as_array().unwrap().len(), 2);
        request(
            13,
            "textDocument/inlayHint",
            json!({"textDocument": {"uri": "file:///tmp/test.s"},
            "range": {"start": {"line": 0, "character": 0}, "end": {"line": 9, "character": 0}}}),
        );
        assert_eq!(response().response_result.unwrap()[0]["label"], json!("exit(status)"));
        request(
            14,
            "textDocument/documentSymbol",
            json!({"textDocument": {"uri": "file:///tmp/test.s"}}),
        );
        assert_eq!(response().response_result.unwrap()[0]["name"], json!("f"));
        request(15, "textDocument/rename", {
            let mut params = at(3, 8);
            params["newName"] = json!("start");
            params
        });
        let edit = response().response_result.unwrap();
        assert_eq!(edit["changes"]["file:///tmp/test.s"].as_array().unwrap().len(), 2);
        request(16, "textDocument/completion", at(3, 8));
        assert!(response().response_result.unwrap().as_array().unwrap().len() > 60);

        request(2, "textDocument/foldingRange", json!({}));
        assert_eq!(response().response_result.unwrap_err().code, ErrorCode::MethodNotFound as i32);

        notify("textDocument/didClose", json!({"textDocument": {"uri": "file:///tmp/test.s"}}));
        assert!(diagnostics().is_empty());

        request(3, "shutdown", json!(null));
        response();
        notify("exit", json!(null));
        handle.join().unwrap();
    }
}
