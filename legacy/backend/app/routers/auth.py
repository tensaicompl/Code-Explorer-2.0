"""Signing in with the shared development credential.
"""
from __future__ import annotations

import logging
import secrets

from fastapi import APIRouter, HTTPException
from pydantic import BaseModel

from .. import config
from .guard import LOGIN_SESSION_TOKEN

logger = logging.getLogger("praxevia.routers.auth")

router = APIRouter()

class LoginCredentials(BaseModel):
    username: str
    password: str


@router.post("/api/login")
async def login(body: LoginCredentials):
    # Belt and braces with the startup guard below. If the credentials are
    # unset, `"" == ""` would authenticate an empty form — strictly worse than
    # the committed default this replaced. Never compare against an empty
    # configured secret.
    if not config.LOGIN_USERNAME or not config.LOGIN_PASSWORD:
        raise HTTPException(
            status_code=503,
            detail="Sign-in is not configured. Set LOGIN_USERNAME and "
                   "LOGIN_PASSWORD on the backend (see .env.example).",
        )
    if secrets.compare_digest(body.username, config.LOGIN_USERNAME) and \
            secrets.compare_digest(body.password, config.LOGIN_PASSWORD):
        return {"token": LOGIN_SESSION_TOKEN}
    raise HTTPException(status_code=401, detail="Invalid credentials")
