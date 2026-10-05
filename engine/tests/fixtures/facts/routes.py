from fastapi import FastAPI

app = FastAPI()


@app.get("/users/{user_id}")
def read_user(user_id: int) -> dict:
    return {"id": user_id}


@app.post("/users")
def create_user(name: str, admin: bool = False) -> dict:
    return {"name": name, "admin": admin}


@app.get("/health")
@app.post("/status")
def health() -> dict:
    return {}
