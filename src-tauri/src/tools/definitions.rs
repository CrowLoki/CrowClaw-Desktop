use serde_json::json;

use crate::agent::ToolDefinition;

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
    ]
}

#[cfg(test)]
mod tests {
    use super::builtin_tool_definitions;

    #[test]
    fn exposes_only_the_three_approval_gated_alpha_tools() {
        let names = builtin_tool_definitions()
            .into_iter()
            .map(|definition| definition.name)
            .collect::<Vec<_>>();
        assert_eq!(names, ["list_directory", "read_text_file", "run_command"]);
    }
}
