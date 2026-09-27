"""Administering the credentials used to reach git hosts.
"""
from __future__ import annotations

import logging

from fastapi import APIRouter, HTTPException, Request

from ..schemas import CredentialCreateRequest, CredentialUpdateRequest
from ..storage import check_credential_connectivity, list_registered_credentials, modify_credential, register_credential, remove_credential
from .guard import ensure_admin, identify_caller, to_json_safe

logger = logging.getLogger("praxevia.routers.admin_credentials")

router = APIRouter()

@router.get("/api/admin/credentials")
async def admin_list_credentials(http_request: Request):
    user_identity = identify_caller(http_request)
    ensure_admin(user_identity)
    creds = list_registered_credentials()
    for c in creds:
        for k in ("created_at", "updated_at"):
            if c.get(k):
                c[k] = c[k].isoformat() if hasattr(c[k], "isoformat") else str(c[k])
    return {"credentials": creds}


@router.post("/api/admin/credentials")
async def admin_create_credential(body: CredentialCreateRequest, http_request: Request):
    user_identity = identify_caller(http_request)
    ensure_admin(user_identity)
    result = register_credential(
        name=body.name, source_type=body.source_type, base_url=body.base_url,
        token=body.token or "", ssl_verify=body.ssl_verify, created_by=user_identity,
        auth_type=body.auth_type, ssh_key=body.ssh_key or "",
    )
    if not result:
        raise HTTPException(status_code=409, detail="Credential name already exists")
    return {"ok": True, "credential": to_json_safe(result)}


@router.put("/api/admin/credentials/{credential_id}")
async def admin_update_credential(credential_id: str, body: CredentialUpdateRequest, http_request: Request):
    user_identity = identify_caller(http_request)
    ensure_admin(user_identity)
    if not modify_credential(
        credential_id, name=body.name, base_url=body.base_url,
        token=body.token, ssl_verify=body.ssl_verify,
        auth_type=body.auth_type, ssh_key=body.ssh_key,
    ):
        raise HTTPException(status_code=404, detail="Credential not found")
    return {"ok": True}


@router.delete("/api/admin/credentials/{credential_id}")
async def admin_delete_credential(credential_id: str, http_request: Request):
    user_identity = identify_caller(http_request)
    ensure_admin(user_identity)
    if not remove_credential(credential_id):
        raise HTTPException(status_code=400, detail="Credential not found or in use by repository sources")
    return {"ok": True}


@router.post("/api/admin/credentials/{credential_id}/test")
async def admin_test_credential(credential_id: str, http_request: Request):
    user_identity = identify_caller(http_request)
    ensure_admin(user_identity)
    return check_credential_connectivity(credential_id)


# ── Admin: Managed Projects ─────────────────────────────────────────
