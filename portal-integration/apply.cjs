// Idempotent local source patch. Does not deploy or restart services.
const fs = require('node:fs');
const path = require('node:path');
const crypto = require('node:crypto');
const root = path.resolve(process.argv[2] || '');
if (!process.argv[2]) throw new Error('Usage: node portal-integration/apply.cjs /absolute/path/to/sub2api');
const router = path.join(root,'frontend/src/router/index.ts');
const routes = path.join(root,'backend/internal/server/routes/auth.go');
const view = path.join(root,'frontend/src/views/auth/ToolboxAuthorizeView.vue');
const handler = path.join(root,'backend/internal/handler/toolbox_auth_handler.go');
const handlerTest = path.join(root,'backend/internal/handler/toolbox_auth_handler_test.go');
const anchor = 'const routes: RouteRecordRaw[] = [';
const apiAnchor = 'authenticated.GET("/auth/me", h.Auth.GetCurrentUser)';
const publicAnchor = 'auth := v1.Group("/auth")';
const security = path.join(root,'backend/internal/server/middleware/security_headers.go');
let routerText = fs.readFileSync(router,'utf8');
let apiText = fs.readFileSync(routes,'utf8');
let securityText = fs.readFileSync(security,'utf8');
if (!routerText.includes(anchor) || !apiText.includes(apiAnchor) || !apiText.includes(publicAnchor) || !securityText.includes('finalPolicy := policy')) throw new Error('Unsupported source version; expected route anchors not found. No changes made.');
const additions = [
  [view, fs.readFileSync(path.join(__dirname,'ToolboxAuthorizeView.vue'),'utf8')],
  [handler, fs.readFileSync(path.join(__dirname,'toolbox_auth_handler.go'),'utf8')],
  [handlerTest, fs.readFileSync(path.join(__dirname,'toolbox_auth_handler_test.go'),'utf8')],
];
if (!routerText.includes("path: '/toolbox/authorize'")) routerText=routerText.replace(anchor,anchor+`\n  {\n    path: '/toolbox/authorize',\n    name: 'ToolboxAuthorize',\n    component: () => import('@/views/auth/ToolboxAuthorizeView.vue'),\n    meta: { requiresAuth: true, title: 'NFTAI 工具箱授权' }\n  },`);
if (!apiText.includes('h.Auth.CreateToolboxSession')) apiText=apiText.replace(apiAnchor,apiAnchor+'\n\t\tauthenticated.POST("/auth/toolbox-session", h.Auth.CreateToolboxSession)');
if (!apiText.includes('h.Auth.ToolboxCapabilities')) apiText=apiText.replace(publicAnchor,publicAnchor+'\n\tauth.GET("/toolbox-capabilities", h.Auth.ToolboxCapabilities)');
if (!securityText.includes('NFTAI toolbox loopback authorization')) securityText=securityText.replace('finalPolicy := policy',`finalPolicy := policy
        // NFTAI toolbox loopback authorization: relax only this page's form target.
        if c.Request.URL.Path == "/toolbox/authorize" {
            finalPolicy = addToDirective(finalPolicy, "form-action", "http://127.0.0.1:*")
        }`);
additions.push([router,routerText],[routes,apiText],[security,securityText]);
const backup = path.join(root,'.nftai-toolbox-backups',new Date().toISOString().replace(/[:.]/g,'-'));
const changed=additions.filter(([target,content])=>!fs.existsSync(target)||fs.readFileSync(target,'utf8')!==content);
if(changed.length){
  fs.mkdirSync(backup,{recursive:true});
  const manifest=[];
  for(const [target,content] of changed){
    const relative=path.relative(root,target);
    const original=fs.existsSync(target)?fs.readFileSync(target):null;
    if(original){const dest=path.join(backup,relative);fs.mkdirSync(path.dirname(dest),{recursive:true});fs.writeFileSync(dest,original);}
    manifest.push({path:relative,existed:!!original,sha256:original?crypto.createHash('sha256').update(original).digest('hex'):null,appliedSha256:crypto.createHash('sha256').update(content).digest('hex')});
    fs.mkdirSync(path.dirname(target),{recursive:true});fs.writeFileSync(target,content);
  }
  fs.writeFileSync(path.join(backup,'manifest.json'),JSON.stringify(manifest,null,2));
}
console.log(changed.length?`Applied ${changed.length} files; backup: ${backup}`:'Already applied; no changes.');
