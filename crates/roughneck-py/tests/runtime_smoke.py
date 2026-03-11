import roughneck_py


def log_hook(payload):
    print("python hook registered")
    return None


def lookup_release(payload):
    print("python tool registered", payload)
    return {"version": "0.1.0"}


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
session = agent.start_session(
    {
        "initial_messages": [
            {"role": "user", "content": "hello"},
        ]
    }
)
assert session.session_id
print("python runtime smoke passed")
