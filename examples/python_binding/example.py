import roughneck_py


def log_hook(payload):
    print("python hook payload:", payload)
    return None


agent = roughneck_py.create_deep_agent({})
agent.register_hook("notification", log_hook)
session = agent.start_session({})
response = session.invoke(
    {
        "messages": [
            {"role": "user", "content": "Summarize the workspace"},
        ]
    }
)
print(response)
