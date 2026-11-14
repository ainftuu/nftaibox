// One-off migration for the first local backup, created before appliedSha256 existed.
// Reconstruct the patch from backed-up originals and compare every resulting byte.
const fs=require('node:fs');
const path=require('node:path');
const os=require('node:os');
const crypto=require('node:crypto');
const {execFileSync}=require('node:child_process');
const root=path.resolve(process.argv[2]);
const backup=path.resolve(process.argv[3]);
if(!backup.startsWith(path.join(root,'.nftai-toolbox-backups')+path.sep))throw Error('Invalid backup');
const manifestPath=path.join(backup,'manifest.json');
const manifest=JSON.parse(fs.readFileSync(manifestPath,'utf8'));
const temp=fs.mkdtempSync(path.join(os.tmpdir(),'nftai-backup-check-'));
const hash=b=>crypto.createHash('sha256').update(b).digest('hex');
try{
  for(const item of manifest){
    const dest=path.resolve(temp,item.path);
    const saved=path.resolve(backup,item.path);
    if(!dest.startsWith(temp+path.sep)||!saved.startsWith(backup+path.sep))throw Error('Invalid path');
    if(item.existed){const bytes=fs.readFileSync(saved);if(hash(bytes)!==item.sha256)throw Error('Original checksum mismatch');fs.mkdirSync(path.dirname(dest),{recursive:true});fs.writeFileSync(dest,bytes);}
  }
  execFileSync(process.execPath,[path.join(__dirname,'apply.cjs'),temp],{stdio:'pipe'});
  for(const item of manifest){
    const expected=fs.readFileSync(path.join(temp,item.path));
    if(!expected.equals(fs.readFileSync(path.join(root,item.path))))throw Error(`Subsequent changes: ${item.path}`);
    item.appliedSha256=hash(expected);
  }
  fs.writeFileSync(manifestPath,JSON.stringify(manifest,null,2));
  console.log('All five patched files match reconstruction; rollback manifest upgraded.');
}finally{fs.rmSync(temp,{recursive:true,force:true});}
