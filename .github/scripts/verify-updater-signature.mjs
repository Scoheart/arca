#!/usr/bin/env node
// Minisign Ed/ED packet format matches Tauri's minisign-verify implementation.
// Verify both the payload and trusted comment using Node's built-in crypto.
import { createHash, createPublicKey, verify } from 'node:crypto';
import { createReadStream } from 'node:fs';
import { readFile } from 'node:fs/promises';

const [manifestPath, configPath, payloadPath] = process.argv.slice(2);
if (!payloadPath) throw new Error('Usage: verify-updater-signature.mjs <latest.json> <tauri.conf.json> <updater>');
const manifest = JSON.parse(await readFile(manifestPath, 'utf8'));
const config = JSON.parse(await readFile(configPath, 'utf8'));
function decode(value) {
  if (typeof value !== 'string' || !/^[A-Za-z0-9+/]*={0,2}$/.test(value) || value.length % 4) throw new Error('Invalid base64');
  const result = Buffer.from(value, 'base64');
  if (result.toString('base64') !== value) throw new Error('Noncanonical base64');
  return result;
}
const publicLines = decode(config.plugins.updater.pubkey).toString('utf8').trimEnd().split(/\r?\n/);
const signatureLines = decode(manifest.platforms['darwin-aarch64'].signature).toString('utf8').trimEnd().split(/\r?\n/);
if (publicLines.length !== 2 || signatureLines.length !== 4 || !signatureLines[2].startsWith('trusted comment: ')) throw new Error('Invalid minisign packet');
const publicPacket = decode(publicLines[1]);
const packet = decode(signatureLines[1]);
const globalSignature = decode(signatureLines[3]);
if (publicPacket.length !== 42 || packet.length !== 74 || globalSignature.length !== 64 || publicPacket.subarray(0, 2).toString() !== 'Ed') throw new Error('Invalid minisign key or signature size');
if (!publicPacket.subarray(2, 10).equals(packet.subarray(2, 10))) throw new Error('Updater signing key ID mismatch');
const key = createPublicKey({ key: Buffer.concat([Buffer.from('302a300506032b6570032100', 'hex'), publicPacket.subarray(10)]), format: 'der', type: 'spki' });
const algorithm = packet.subarray(0, 2).toString();
let message;
if (algorithm === 'ED') {
  const hash = createHash('blake2b512');
  for await (const chunk of createReadStream(payloadPath)) hash.update(chunk);
  message = hash.digest();
} else if (algorithm === 'Ed') {
  message = await readFile(payloadPath);
} else throw new Error('Unsupported minisign algorithm');
const signature = packet.subarray(10);
if (!verify(null, message, key, signature)) throw new Error('Updater payload signature is invalid');
const trusted = Buffer.from(signatureLines[2].slice('trusted comment: '.length));
if (!verify(null, Buffer.concat([signature, trusted]), key, globalSignature)) throw new Error('Updater trusted comment signature is invalid');
console.log('Updater payload and trusted comment signatures: PASS');
