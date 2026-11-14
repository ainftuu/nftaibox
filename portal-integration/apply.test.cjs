const {test}=require('node:test');
const assert=require('node:assert/strict');
const fs=require('node:fs');
const os=require('node:os');
const path=require('node:path');
const {execFileSync}=require('node:child_process');
function fixture(){
  const root=fs.mkdtempSync(path.join(os.tmpdir(),'nftai-portal-test-'));
  const files={
    'frontend/src/router/index.ts':'const routes: RouteRecordRaw[] = [\n];',
    'backend/internal/server/routes/auth.go':'auth := v1.Group("/auth")\nauthenticated.GET("/auth/me", h.Auth.GetCurrentUser)',
    'backend/internal/server/middleware/security_headers.go':'finalPolicy := policy',
  };
  for(const [name,content]of Object.entries(files)){const p=path.join(root,name);fs.mkdirSync(path.dirname(p),{recursive:true});fs.writeFileSync(p,content);}
  return {root,files};
}
const run=(script,args)=>execFileSync(process.execPath,[path.join(__dirname,script),...args],{encoding:'utf8',stdio:['ignore','pipe','pipe']});

test('loopback referrer policy is scoped to authorization and idempotent',()=>{
  const root=fs.mkdtempSync(path.join(os.tmpdir(),'nftai-caddy-'));
  const file=path.join(root,'Caddyfile');
  fs.writeFileSync(file,'api.nftai.fun {\n reverse_proxy sub2api:8080\n}\n');
  run('apply-caddy.cjs',[file]);run('apply-caddy.cjs',[file]);
  const text=fs.readFileSync(file,'utf8');
  assert.equal((text.match(/NFTAI_TOOLBOX_REFERRER/g)||[]).length,1);
  assert.match(text,/@toolboxAuth path \/toolbox\/authorize/);
  assert.match(text,/header @toolboxAuth >Referrer-Policy origin/);
  fs.writeFileSync(file,'unexpected');
  assert.throws(()=>run('apply-caddy.cjs',[file]));
  assert.equal(fs.readFileSync(file,'utf8'),'unexpected');
});
test('source patch is idempotent and rollback restores original files',()=>{
  const {root,files}=fixture();
  run('apply.cjs',[root]);
  assert.match(run('apply.cjs',[root]),/Already applied/);
  const backups=fs.readdirSync(path.join(root,'.nftai-toolbox-backups'));
  assert.equal(backups.length,1);
  const security=fs.readFileSync(path.join(root,'backend/internal/server/middleware/security_headers.go'),'utf8');
  assert.match(security,/c.Request.URL.Path == "\/toolbox\/authorize"/);
  const routes=fs.readFileSync(path.join(root,'backend/internal/server/routes/auth.go'),'utf8');
  assert.equal((routes.match(/h.Auth.ToolboxCapabilities/g)||[]).length,1);
  assert.equal((routes.match(/h.Auth.CreateToolboxSession/g)||[]).length,1);
  run('rollback.cjs',[root,path.join(root,'.nftai-toolbox-backups',backups[0])]);
  for(const [name,content]of Object.entries(files))assert.equal(fs.readFileSync(path.join(root,name),'utf8'),content);
  assert.equal(fs.existsSync(path.join(root,'backend/internal/handler/toolbox_auth_handler.go')),false);
});
test('rollback refuses to overwrite subsequent user edits',()=>{
  const {root}=fixture();run('apply.cjs',[root]);
  const file=path.join(root,'frontend/src/router/index.ts');fs.appendFileSync(file,'\n// later edit');
  const [name]=fs.readdirSync(path.join(root,'.nftai-toolbox-backups'));
  assert.throws(()=>run('rollback.cjs',[root,path.join(root,'.nftai-toolbox-backups',name)]));
  assert.match(fs.readFileSync(file,'utf8'),/later edit/);
});
test('unsupported source layout fails before creating authorization files',()=>{
  const {root}=fixture();fs.writeFileSync(path.join(root,'frontend/src/router/index.ts'),'unsupported');
  assert.throws(()=>run('apply.cjs',[root]));
  assert.equal(fs.existsSync(path.join(root,'backend/internal/handler/toolbox_auth_handler.go')),false);
});
test('future theme rebuild hook is idempotent and refuses unsupported installers',()=>{
  const root=fs.mkdtempSync(path.join(os.tmpdir(),'nftai-theme-hook-'));
  const file=path.join(root,'apply-theme.sh');
  fs.writeFileSync(file,'#!/bin/bash\necho "==> 5/6 构建镜像"\n');
  run('persist-theme-hook.cjs',[file]);run('persist-theme-hook.cjs',[file]);
  assert.equal((fs.readFileSync(file,'utf8').match(/NFTAI_TOOLBOX_AUTH_HOOK/g)||[]).length,1);
  fs.writeFileSync(file,'unsupported');
  assert.throws(()=>run('persist-theme-hook.cjs',[file]));
  assert.equal(fs.readFileSync(file,'utf8'),'unsupported');
});
