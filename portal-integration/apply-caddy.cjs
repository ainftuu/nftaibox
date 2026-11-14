// Preserve a verifiable Origin on HTTPS -> loopback HTTP form navigation.
const fs = require('node:fs');
const path = require('node:path');
const file = process.argv[2];
if (!file) throw new Error('Usage: node apply-caddy.cjs /path/to/Caddyfile');
const original = fs.readFileSync(file, 'utf8');
if (original.includes('NFTAI_TOOLBOX_REFERRER')) process.exit(0);
const anchor = 'api.nftai.fun {';
if (original.split(anchor).length !== 2) throw new Error('Unexpected Caddy layout; no changes made');
fs.copyFileSync(file, file + '.before-toolbox-' + Date.now());
// Write in place: the server may bind-mount this exact inode into Caddy.
fs.writeFileSync(file, original.replace(anchor, `${anchor}
    # NFTAI_TOOLBOX_REFERRER: send only the site origin to the loopback form.
    @toolboxAuth path /toolbox/authorize
    header @toolboxAuth >Referrer-Policy origin`));
