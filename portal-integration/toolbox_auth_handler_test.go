package handler

import (
    "encoding/json"
    "net/http/httptest"
    "testing"

    "github.com/gin-gonic/gin"
)

func TestToolboxCapabilities(t *testing.T) {
    recorder := httptest.NewRecorder()
    c, _ := gin.CreateTestContext(recorder)
    h := &AuthHandler{}
    h.ToolboxCapabilities(c)
    if recorder.Code != 200 || recorder.Header().Get("Cache-Control") != "no-store" {
        t.Fatalf("unexpected response status/cache policy: %d", recorder.Code)
    }
    var body struct {
        Code int `json:"code"`
        Data struct {
            Protocol int `json:"protocol"`
            AuthorizePath string `json:"authorize_path"`
            SessionPath string `json:"session_path"`
        } `json:"data"`
    }
    if err := json.Unmarshal(recorder.Body.Bytes(), &body); err != nil { t.Fatal(err) }
    if body.Code != 0 || body.Data.Protocol != 1 || body.Data.AuthorizePath != "/toolbox/authorize" || body.Data.SessionPath != "/api/v1/auth/toolbox-session" {
        t.Fatal("unexpected toolbox capability contract")
    }
}
