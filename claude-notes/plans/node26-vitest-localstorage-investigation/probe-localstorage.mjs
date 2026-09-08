const d = Object.getOwnPropertyDescriptor(globalThis, 'localStorage');
console.log('version', process.version);
console.log('descriptor', d ? {get: !!d.get, set: !!d.set, writable: d.writable, configurable: d.configurable, enumerable: d.enumerable, hasValue: 'value' in d} : 'NONE');
console.log('typeof localStorage', typeof localStorage);
let v; try { v = globalThis.localStorage; console.log('read ->', v === undefined ? 'undefined' : typeof v); } catch (e) { console.log('read throws', e.message); }
try { globalThis.localStorage = { clear(){}, tag: 'assigned' }; console.log('after assign ->', globalThis.localStorage && globalThis.localStorage.tag); } catch (e) { console.log('assign throws', e.message); }
try { Object.defineProperty(globalThis, 'localStorage', { value: { tag: 'defined' }, configurable: true, writable: true }); console.log('after defineProperty ->', globalThis.localStorage.tag); } catch (e) { console.log('defineProperty throws', e.message); }
