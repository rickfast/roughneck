from __future__ import annotations

from typing import Optional

import roughneck_py as rpy


def log_hook(payload: rpy.HookPayloadDict) -> Optional[rpy.HookDecisionDict]:
    event: rpy.HookEventPayloadName = payload["hook_event_name"]
    if event == "Notification":
        return {"messages": ["typed python hook"]}
    return None


config: rpy.DeepAgentConfigDict = {
    "system_prompt": "typed smoke",
    "subagents": {"status": "disabled", "agents": []},
}
agent: rpy.DeepAgent = rpy.create_deep_agent(config)
agent.register_hook("notification", log_hook)
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
