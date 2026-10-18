// A throwaway OpenTimestamps calendar for tests: never a real calendar, never on the network.
// Answers POST /digest with a pending attestation (its own URL), as public calendars do before
// Bitcoin confirms, and GET /timestamp/<hex> with 404 ("pending"). Prints its URL as the first line.
//   node tools/local-ots-calendar.mjs
import { createServer } from "node:http";

const PENDING = Buffer.from("83dfe30d2ef90c8e", "hex");
const varbytes = (b) => Buffer.concat([Buffer.from([b.length]), b]); // every field here is < 128 bytes

const server = createServer((req, res) => {
  const chunks = [];
  req.on("data", (c) => chunks.push(c));
  req.on("end", () => {
    const body = Buffer.concat(chunks);
    if (req.method === "POST" && req.url === "/digest" && body.length === 32) {
      const time = Buffer.alloc(4);
      time.writeUInt32BE(Math.floor(Date.now() / 1000));
      const url = Buffer.from(`http://127.0.0.1:${server.address().port}`);
      // prepend <time>, then the pending attestation
      res.end(Buffer.concat([Buffer.from([0xf1]), varbytes(time), Buffer.from([0x00]), PENDING, varbytes(varbytes(url))]));
    } else {
      res.writeHead(404).end("Pending confirmation in Bitcoin blockchain");
    }
  });
});
server.listen(0, "127.0.0.1", () => console.log(`http://127.0.0.1:${server.address().port}`));
