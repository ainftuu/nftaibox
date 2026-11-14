const fs=require('node:fs');
const path=require('node:path');
const crypto=require('node:crypto');
if(!process.argv[2]||!process.argv[3]) throw new Error('Usage: node rollback.cjs /path/to/sub2api /path/to/.nftai-toolbox-backups/TIMESTAMP');
const root=fs.realpathSync(process.argv[2]);
const backup=fs.realpathSync(process.argv[3]);
const backupRoot=path.join(root,'.nftai-toolbox-backups');
if(!backup.startsWith(backupRoot+path.sep))throw new Error('Backup must be inside the selected source tree.');
const hash=bytes=>crypto.createHash('sha256').update(bytes).digest('hex');
const manifest=JSON.parse(fs.readFileSync(path.join(backup,'manifest.json'),'utf8'));
const actions=manifest.map(item=>{
  const target=path.resolve(root,item.path);
  const saved=path.resolve(backup,item.path);
  if(!target.startsWith(root+path.sep)||!saved.startsWith(backup+path.sep))throw new Error('Invalid backup path.');
  if(!item.appliedSha256||!fs.existsSync(target)||hash(fs.readFileSync(target))!==item.appliedSha256)throw new Error(`Refusing to overwrite subsequent edits: ${item.path}`);
  const original=item.existed?fs.readFileSync(saved):null;
  if(original&&hash(original)!==item.sha256)throw new Error(`Backup checksum mismatch: ${item.path}`);
  return {target,original};
});
// All files have been checked before the first write.
for(const {target,original} of actions){if(original)fs.writeFileSync(target,original);else fs.unlinkSync(target);}
console.log(`Restored ${actions.length} source files. No services were restarted.`);
