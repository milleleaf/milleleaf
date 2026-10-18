// A throwaway RFC 3161 time-stamp authority for tests: never a real TSA, never on the network.
// Makes a test CA and a TSA certificate (ECDSA P-256) in <dir> with the openssl CLI, then answers
// POSTed time-stamp queries with `openssl ts -reply` on 127.0.0.1. Prints its URL as the first line.
//   node tools/local-tsa.mjs <dir>
// <dir>/ca.pem is the CA to check tokens against (`openssl ts -verify -CAfile`).
import { execFileSync } from "node:child_process";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { createServer } from "node:http";
import { join, resolve } from "node:path";

if (!process.argv[2]) { console.error("usage: node tools/local-tsa.mjs <dir>"); process.exit(2); }
const dir = resolve(process.argv[2]);
mkdirSync(dir, { recursive: true });
const ssl = (...a) => execFileSync("openssl", a, { cwd: dir, stdio: ["ignore", "ignore", "pipe"] });
const ec = ["-newkey", "ec", "-pkeyopt", "ec_paramgen_curve:P-256", "-nodes"];
ssl("req", "-x509", ...ec, "-keyout", "ca.key", "-out", "ca.pem", "-days", "2", "-subj", "/CN=Milleleaf test CA",
  "-addext", "basicConstraints=critical,CA:TRUE", "-addext", "keyUsage=critical,keyCertSign");
ssl("req", ...ec, "-keyout", "tsa.key", "-out", "tsa.csr", "-subj", "/CN=Milleleaf test TSA");
writeFileSync(join(dir, "ext.cnf"), "extendedKeyUsage=critical,timeStamping\nkeyUsage=critical,digitalSignature\n");
ssl("x509", "-req", "-in", "tsa.csr", "-CA", "ca.pem", "-CAkey", "ca.key", "-CAcreateserial", "-out", "tsa.pem",
  "-days", "2", "-extfile", "ext.cnf");
writeFileSync(join(dir, "serial"), "01\n");
// accuracy, ordering and tsa_name make the token carry TSTInfo's optional fields too.
writeFileSync(join(dir, "tsa.cnf"), `[ tsa ]
default_tsa = t
[ t ]
dir = ${dir}
serial = $dir/serial
crypto_device = builtin
signer_cert = $dir/tsa.pem
certs = $dir/ca.pem
signer_key = $dir/tsa.key
signer_digest = sha256
default_policy = 1.2.3.4.1
digests = sha256, sha384, sha512
accuracy = secs:1
ordering = yes
tsa_name = yes
ess_cert_id_chain = no
ess_cert_id_alg = sha256
`);

let n = 0;
const server = createServer((req, res) => {
  const parts = [];
  req.on("data", (c) => parts.push(c));
  req.on("end", () => {
    const q = join(dir, `q${++n}.tsq`), r = join(dir, `r${n}.tsr`);
    try {
      writeFileSync(q, Buffer.concat(parts));
      ssl("ts", "-reply", "-config", join(dir, "tsa.cnf"), "-queryfile", q, "-out", r);
      res.writeHead(200, { "Content-Type": "application/timestamp-reply" }).end(readFileSync(r));
    } catch (e) {
      res.writeHead(500).end(String(e.stderr || e));
    }
  });
});
server.listen(0, "127.0.0.1", () => console.log(`http://127.0.0.1:${server.address().port}/tsa`));
