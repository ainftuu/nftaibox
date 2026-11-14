const fs=require('node:fs');
const file=process.argv[2];
let text=fs.readFileSync(file,'utf8');
if(!text.includes('NFTAI_TOOLBOX_AUTH_HOOK')){
  const anchor=/^echo "==> 5\/6 .*$/m;
  if(!anchor.test(text))throw Error('Unsupported theme installer; refusing to change it.');
  text=text.replace(anchor,match=>`# NFTAI_TOOLBOX_AUTH_HOOK: retain authorization on later theme rebuilds.
docker run --rm -v "$THEME_DIR":/theme:ro -v "$SRC_DIR":/src node:24-alpine node /theme/portal-integration/apply.cjs /src

${match}`);
  fs.writeFileSync(file,text);
}
console.log('Future theme builds retain toolbox authorization.');
