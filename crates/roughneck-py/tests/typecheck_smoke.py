from __future__ import annotations

from typing import Optional

import roughneck_py as rpy


def log_hook(payload: rpy.HookPayloadDict) -> Optional[rpy.HookDecisionDict]:
    event: rpy.HookEventPayloadName = payload["hook_event_name"]
    if event == "Notification":
        return {"messages": ["typed python hook"]}
    return None


def lookup_release(payload: rpy.JsonValue) -> Optional[rpy.JsonValue]:
    if isinstance(payload, dict) and payload.get("name") == "roughneck":
        return {"version": "0.1.0"}
    return None


config: rpy.DeepAgentConfigDict = {
    "system_prompt": "typed smoke",
    "subagents": {"status": "disabled", "agents": []},
}
agent: rpy.DeepAgent = rpy.create_deep_agent(config)
agent.register_hook("notification", log_hook)
agent.register_tool(
    "lookup_release",
    "Return a canned release version for a package name.",
    {
        "type": "object",
        "properties": {"name": {"type": "string"}},
        "required": ["name"],
    },
    lookup_release,
)
session: rpy.AgentSession = agent.start_session(
    {
        "initial_messages": [
            {"role": "user", "content": "hello"},
        ]
    }
)
response: rpy.SessionInvokeResponseDict = session.invoke(
    {
        "messages": [
            {"role": "user", "content": "world"},
        ]
    }
)
latest = response.get("latest_assistant_message")
if latest is not None:
    role: rpy.RoleName = latest["role"]
    print(role)
