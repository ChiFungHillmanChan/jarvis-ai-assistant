// Just enough DOM for react-dom 18 createRoot and updates under node:test.
// No layout, styles or event dispatch: tests use it to see which nodes React
// creates and which ones it keeps. Import it before react-dom.
class Node {
  nodeType: number; nodeName: string; ownerDocument: Document | null;
  parentNode: Node | null = null;
  childNodes: Node[] = [];
  constructor(nodeType: number, nodeName: string, ownerDocument: Document | null) {
    this.nodeType = nodeType; this.nodeName = nodeName; this.ownerDocument = ownerDocument;
  }
  get firstChild() { return this.childNodes[0] || null; }
  get lastChild() { return this.childNodes[this.childNodes.length - 1] || null; }
  appendChild(child: Node) { child.parentNode?.removeChild(child); child.parentNode = this; this.childNodes.push(child); return child; }
  insertBefore(child: Node, ref: Node | null) {
    child.parentNode?.removeChild(child);
    const at = ref ? this.childNodes.indexOf(ref) : -1;
    child.parentNode = this;
    if (at < 0) this.childNodes.push(child); else this.childNodes.splice(at, 0, child);
    return child;
  }
  removeChild(child: Node) { const at = this.childNodes.indexOf(child); if (at >= 0) this.childNodes.splice(at, 1); child.parentNode = null; return child; }
  addEventListener() {}
  removeEventListener() {}
  contains(node: Node | null) { for (let n = node; n; n = n.parentNode) if (n === this) return true; return false; }
  get textContent(): string { return this.childNodes.map(child => child.textContent).join(''); }
  set textContent(value: string) {
    for (const child of this.childNodes) child.parentNode = null;
    this.childNodes = [];
    if (value) this.appendChild(new Text(value, this.ownerDocument));
  }
}
class Text extends Node {
  nodeValue: string;
  constructor(value: string, doc: Document | null) { super(3, '#text', doc); this.nodeValue = value; }
  get textContent() { return this.nodeValue; }
  set textContent(value: string) { this.nodeValue = value; }
}
export class Element extends Node {
  tagName: string;
  namespaceURI = 'http://www.w3.org/1999/xhtml';
  attributes: Record<string, string> = {};
  style: Record<string, string> = {};
  constructor(tag: string, doc: Document | null) { super(1, tag.toUpperCase(), doc); this.tagName = tag.toUpperCase(); }
  setAttribute(name: string, value: unknown) { this.attributes[name] = String(value); }
  setAttributeNS(_ns: string, name: string, value: unknown) { this.attributes[name] = String(value); }
  removeAttribute(name: string) { delete this.attributes[name]; }
  getAttribute(name: string) { return name in this.attributes ? this.attributes[name] : null; }
}
class Document extends Node {
  documentElement: Element; body: Element; activeElement: Element;
  constructor() {
    super(9, '#document', null);
    this.documentElement = new Element('html', this);
    this.body = new Element('body', this);
    this.documentElement.appendChild(this.body);
    this.activeElement = this.body;
  }
  createElement(tag: string) { return new Element(tag, this); }
  createElementNS(ns: string, tag: string) { const element = new Element(tag, this); element.namespaceURI = ns; return element; }
  createTextNode(value: string) { return new Text(value, this); }
}

export const document = new Document();
Object.assign(globalThis, { window: globalThis, document, HTMLIFrameElement: class {} });

/** Every element with this tag name under root, in document order. */
export function byTag(root: Node, tag: string): Element[] {
  const found: Element[] = [];
  const walk = (node: Node) => { if (node instanceof Element && node.tagName === tag.toUpperCase()) found.push(node); node.childNodes.forEach(walk); };
  walk(root);
  return found;
}
