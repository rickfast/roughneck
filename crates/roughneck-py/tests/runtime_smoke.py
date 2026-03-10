import roughneck_py


def log_hook(payload):
    print("python hook registered")
    return None


agent = roughneck_py.create_deep_agent({})
agent.register_hook("notification", log_hook)
session = agent.start_session(
    {
        "initial_messages": [
            {"role": "user", "content": "hello"},
        ]
    }
)
assert session.session_id
print("python runtime smoke passed")
