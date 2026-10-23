// arming: ci .github/actions/wasm-explorer-smoke/action.yml
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const source = await readFile(new URL("../docs/book/src/explore/entity.js", import.meta.url));
const { renderEntity } = await import(`data:text/javascript;base64,${source.toString("base64")}`);

class Element {
  constructor(tag) { this.tag = tag; this.children = []; this.dataset = {}; }
  append(...children) { this.children.push(...children); }
  replaceChildren(...children) { this.children = children; }
  anchors() {
    return [...(this.tag === "a" ? [this] : []), ...this.children.flatMap((child) => child.anchors())];
  }
}

const filePath = { predicate: "https://quipu.dev/filePath", value: "src/lib.rs" };
const provenance = (value) => ({ predicate: "http://www.w3.org/ns/prov#wasDerivedFrom", value });
const cases = [
  ["filePath fallback", [filePath], "https://github.com/intigraph/quipu/blob/main/src/lib.rs"],
  ["explicit current repository", [provenance("https://github.com/intigraph/quipu/blob/revision/src/lib.rs")],
    "https://github.com/intigraph/quipu/blob/revision/src/lib.rs"],
  ["historical repository", [provenance("https://github.com/scbrown/quipu/blob/revision/src/lib.rs")],
    "https://github.com/scbrown/quipu/blob/revision/src/lib.rs"],
  ["foreign repository despite filePath", [filePath, provenance("https://github.com/intigraph/other/blob/main/src/lib.rs")], null],
  ["unrelated host", [provenance("https://example.org/intigraph/quipu/blob/main/src/lib.rs")], null],
  ["near-miss owner", [provenance("https://github.com/intigraph-extra/quipu/blob/main/src/lib.rs")], null],
  ["near-miss repository", [provenance("https://github.com/scbrown/quipu-extra/blob/main/src/lib.rs")], null],
];

for (const [name, properties, expected] of cases) {
  test(name, () => {
    const previous = globalThis.document;
    globalThis.document = { createElement: (tag) => new Element(tag) };
    try {
      const card = new Element("article");
      renderEntity(card, "urn:fixture:entity", { types: [], properties, links: [] }, {
        short: (value) => value, navigate() {}, inspect() {},
      });
      const links = card.anchors();
      assert.deepEqual(links.map((link) => link.href), expected ? [expected] : []);
      if (expected) {
        assert.equal(links[0].textContent, "Read the source ↗");
        assert.equal(links[0].target, "_blank");
        assert.equal(links[0].rel, "noopener noreferrer");
      }
    } finally {
      if (previous === undefined) delete globalThis.document;
      else globalThis.document = previous;
    }
  });
}
