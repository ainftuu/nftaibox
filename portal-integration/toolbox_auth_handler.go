package handler

import (
    "github.com/gin-gonic/gin"
    "github.com/Wei-Shaw/sub2api/internal/pkg/response"
    middleware2 "github.com/Wei-Shaw/sub2api/internal/server/middleware"
)

// ToolboxCapabilities exposes no account data and detects incomplete deployments.
func (h *AuthHandler) ToolboxCapabilities(c *gin.Context) {
    c.Header("Cache-Control", "no-store")
    response.Success(c, gin.H{
        "protocol": 1,
        "authorize_path": "/toolbox/authorize",
        "session_path": "/api/v1/auth/toolbox-session",
    })
}

// CreateToolboxSession issues an independent token family for the desktop app.
// It must be registered under the same JWT + backend-mode + panel rate limit
// middleware as /auth/me. Never expose this as an unauthenticated endpoint.
func (h *AuthHandler) CreateToolboxSession(c *gin.Context) {
    c.Header("Cache-Control", "no-store")
    subject, ok := middleware2.GetAuthSubjectFromContext(c)
    if !ok {
        response.Unauthorized(c, "User not authenticated")
        return
    }
    user, err := h.userService.GetByID(c.Request.Context(), subject.UserID)
    if err != nil {
        response.ErrorFrom(c, err)
        return
    }
    if user.Status != "active" {
        response.Unauthorized(c, "User account is not active")
        return
    }
    h.respondWithTokenPair(c, user)
}
