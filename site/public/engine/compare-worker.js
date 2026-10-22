import init, { Demo, file_metadata } from './pkg/ox_web.js';
import wasmBytes from './pkg/ox_web_bytes.js';

let demo;
const ready = init({ module_or_path: wasmBytes }).then(() => { demo = new Demo(); });
// Serialize calls: adoption, comparison and signing share one in-memory session.
let queue = Promise.resolve();
self.onmessage = ({ data: { id, method, args = [] } }) => {
  queue = queue.then(async () => {
    try {
      await ready;
      let result;
      switch (method) {
        case 'ready': result = { ready: true }; break;
        case 'metadata': result = file_metadata(new Uint8Array(args[0])); break;
        case 'adopt': result = demo.adopt(new Uint8Array(args[0])); break;
        case 'diff': result = demo.diff(new Uint8Array(args[0]), args[1], args[2]); break;
        case 'sign': result = demo.sign(); break;
        case 'verify': result = demo.verify(new Uint8Array(args[0]), new Date().toISOString()); break;
        default: throw new Error('Unknown document operation.');
      }
      self.postMessage({ id, result: typeof result === 'string' ? JSON.parse(result) : result });
    } catch (error) {
      self.postMessage({ id, error: error instanceof Error ? error.message : String(error) });
    }
  });
};
