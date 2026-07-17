use serde_json::json;

use crate::agent::ToolDefinition;

use super::types::MEMORY_TEXT_MAX_BYTES;

pub fn builtin_tool_definitions() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition {
            name: "list_directory".into(),
            description: "Propose listing one user-selected directory. The list is not read until the user approves the exact path.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Absolute path inside a folder selected by the user"
                    }
                },
                "required": ["path"],
                "additionalProperties": false
            }),
        },
        ToolDefinition {
            name: "read_text_file".into(),
            description: "Propose reading one UTF-8 text file. No file content is read until the user approves the exact path.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Absolute path inside a folder selected by the user"
                    }
                },
                "required": ["path"],
                "additionalProperties": false
            }),
        },
        ToolDefinition {
            name: "run_command".into(),
            description: "Propose running one executable with an explicit argument array and working directory. The command never starts until the user approves the exact proposal.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "program": {
                        "type": "string",
                        "description": "Executable name or path; do not combine a shell pipeline into this field"
                    },
                    "args": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Exact argument vector"
                    },
                    "cwd": {
                        "type": "string",
                        "description": "Absolute working directory inside a folder selected by the user"
                    }
                },
                "required": ["program", "args", "cwd"],
                "additionalProperties": false
            }),
        },
        ToolDefinition {
            name: "remember_memory".into(),
            description: "Propose storing text in CrowClaw's local CrowQuant compressed lexical memory. Nothing is compressed or written until the user approves the exact text.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "text": {
                        "type": "string",
                        "minLength": 1,
                        "maxLength": MEMORY_TEXT_MAX_BYTES,
                        "description": "The exact text to store in local CrowQuant memory"
                    }
                },
                "required": ["text"],
                "additionalProperties": false
            }),
        },
        ToolDefinition {
            name: "search_memory".into(),
            description: "Propose ranking CrowClaw's local CrowQuant memory by compressed lexical similarity. No stored memory is read until the user approves the exact query and result limit.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "minLength": 1,
                        "maxLength": MEMORY_TEXT_MAX_BYTES,
                        "description": "The exact lexical memory query"
                    },
                    "limit": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": 20,
                        "default": 5,
                        "description": "Maximum number of top-ranked results to return; no relevance threshold is applied"
                    }
                },
                "required": ["query"],
                "additionalProperties": false
            }),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::builtin_tool_definitions;

    #[test]
    fn exposes_the_five_approval_gated_crowclaw_tools() {
        let names = builtin_tool_definitions()
            .into_iter()
            .map(|definition| definition.name)
            .collect::<Vec<_>>();
        assert_eq!(
            names,
            [
                "list_directory",
                "read_text_file",
                "run_command",
                "remember_memory",
                "search_memory"
            ]
        );
    }
}
