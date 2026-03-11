import roughneck_py


def log_hook(payload):
    print("python hook payload:", payload)
    return None


def lookup_release(payload):
    if payload.get("name") == "roughneck":
        return {"version": "0.1.0"}
    return None


agent = roughneck_py.create_deep_agent({})
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
session = agent.start_session({})
response = session.invoke(
    {
        "messages": [
            {"role": "user", "content": "Summarize the workspace"},
        ]
    }
)
print(response)
