import {readFileSync,statSync} from 'node:fs';import assert from 'node:assert/strict';
const path=process.env.OM_TEST_EXE||new URL('../target/release/ncm-overlay-control.exe',import.meta.url);const b=readFileSync(path),pe=b.readUInt32LE(60),coff=pe+4,opt=coff+20;
assert.equal(b.toString('ascii',pe,pe+4),'PE\0\0');assert.equal(b.readUInt16LE(coff),0x8664,'Windows x64 required');assert.equal(b.readUInt16LE(opt),0x20b);assert.equal(b.readUInt16LE(opt+68),2,'Executable must have no console');
const sections=coff+20+b.readUInt16LE(coff+16),count=b.readUInt16LE(coff+2);
function offset(rva){for(let i=0;i<count;i++){const s=sections+40*i,virtual=b.readUInt32LE(s+12),size=Math.max(b.readUInt32LE(s+8),b.readUInt32LE(s+16));if(rva>=virtual&&rva<virtual+size)return b.readUInt32LE(s+20)+rva-virtual;}throw Error('Unmapped RVA');}
const imports=[],table=offset(b.readUInt32LE(opt+120));for(let p=table;b.readUInt32LE(p+12)!==0;p+=20){const start=offset(b.readUInt32LE(p+12)),end=b.indexOf(0,start);imports.push(b.toString('ascii',start,end));}
for(const dll of imports)assert.match(dll,/^(?:kernel32|user32|shell32|advapi32|ntdll|ws2_32|userenv|bcrypt|bcryptprimitives|gdi32|ole32|api-ms-win-[a-z0-9-]+)\.dll$/i,'Unexpected external DLL '+dll);
assert.ok(statSync(path).size<1024*1024,'First release should remain below 1 MiB');
console.log(JSON.stringify({bytes:statSync(path).size,architecture:'x64',subsystem:'Windows GUI, no console',systemDlls:imports},null,2));
