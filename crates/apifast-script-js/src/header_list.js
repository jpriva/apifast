// `req.headerList` and `res.headerList`: Bruno's list of headers (its
// `header-list.js`), Postman's PropertyList API, over the headers as
// `[name, value]` pairs. Names match in any case. It leaves itself in
// `__apifast_header_list(read, write)`: `read` gives the pairs, `write`
// replaces them; without `write` the list is read-only, as the response's.
(() => {
    const same = (a, b) => typeof a === 'string' && typeof b === 'string' && a.toLowerCase() === b.toLowerCase();
    const bind = (fn, context) => (context !== undefined ? fn.bind(context) : fn);
    const parse = (text) => {
        if (typeof text !== 'string') return null;
        const colon = text.indexOf(':');
        if (colon === -1) return null;
        return { key: text.slice(0, colon).trim(), value: text.slice(colon + 1).trim() };
    };

    const headerList = (read, write) => {
        const items = () => read().map(([key, value]) => ({ key, value }));
        const writable = () => {
            if (!write) throw new Error('HeaderList is read-only (response headers cannot be modified)');
        };
        const save = (list) => write(list.map(({ key, value }) => [String(key), String(value)]));
        const list = {
            get(name) { return list.one(name)?.value; },
            one(name) { return items().filter((h) => same(h.key, name)).at(-1); },
            all: () => items(),
            idx: (index) => items()[index],
            count: () => items().length,
            indexOf(nameOrItem) {
                if (typeof nameOrItem === 'string') return items().findIndex((h) => same(h.key, nameOrItem));
                if (!nameOrItem || typeof nameOrItem !== 'object') return -1;
                return items().findIndex((h) => same(h.key, nameOrItem.key) && h.value === nameOrItem.value);
            },
            has(nameOrItem, value) {
                const name = nameOrItem && typeof nameOrItem === 'object' ? nameOrItem.key : nameOrItem;
                return items().some((h) => same(h.key, name) && (value === undefined || h.value === value));
            },
            find: (predicate, context) => items().find(bind(predicate, context)),
            filter: (predicate, context) => items().filter(bind(predicate, context)),
            each(fn, context) { items().forEach(bind(fn, context)); },
            map: (fn, context) => items().map(bind(fn, context)),
            reduce(fn, ...rest) {
                const bound = bind(fn, rest[1]);
                return rest.length ? items().reduce(bound, rest[0]) : items().reduce(bound);
            },
            toObject(excludeDisabled, caseSensitive, multiValue, sanitizeKeys) {
                const object = {};
                for (const { key: name, value } of items()) {
                    const key = caseSensitive === false ? name.toLowerCase() : name;
                    if (sanitizeKeys && !key) continue;
                    if (!multiValue || !(key in object)) object[key] = value;
                }
                return object;
            },
            toString() {
                const all = items();
                return all.length === 0 ? '' : `${all.map((h) => `${h.key}: ${h.value}`).join('\n')}\n`;
            },
            toJSON: () => items(),

            // Writes: only the request's.
            add(itemOrName, value) {
                if (typeof itemOrName === 'string' && value !== undefined) return void list.upsert({ key: itemOrName, value });
                const item = typeof itemOrName === 'string' ? parse(itemOrName) : itemOrName;
                list.upsert(item);
            },
            upsert(itemOrName, value) {
                writable();
                const item = typeof itemOrName === 'string' ? { key: itemOrName, value } : itemOrName;
                if (!item || typeof item !== 'object' || !item.key) return null;
                const all = items();
                const first = all.findIndex((h) => same(h.key, item.key));
                const existed = first >= 0;
                if (existed && all[first].key === item.key) {
                    all[first].value = item.value;
                } else {
                    // Another casing of it goes, and the new one is last.
                    if (existed) all.splice(first, 1);
                    all.push({ key: item.key, value: item.value });
                }
                save(all);
                return !existed;
            },
            remove(predicate, context) {
                writable();
                let gone;
                if (typeof predicate === 'function') gone = bind(predicate, context);
                else if (typeof predicate === 'string') gone = (h) => same(h.key, predicate);
                else if (predicate && typeof predicate === 'object' && predicate.key) gone = (h) => same(h.key, predicate.key);
                else return;
                save(items().filter((h) => !gone(h)));
            },
            clear() {
                writable();
                save([]);
            },
            populate(source) {
                writable();
                const given = typeof source === 'string'
                    ? source.split(/\r?\n/).filter((line) => line.trim()).map(parse)
                    : (Array.isArray(source) ? source : []);
                for (const item of given) {
                    if (item && item.key && !list.has(item.key)) list.add(item);
                }
            },
            repopulate(source) {
                list.clear();
                list.populate(source);
            },
            assimilate(source, prune) {
                writable();
                let given = [];
                if (source && typeof source.all === 'function') given = source.all();
                else if (Array.isArray(source)) given = source;
                for (const item of given) list.add(item);
                if (prune && given.length > 0) {
                    const keep = new Set(given.map((i) => String(i.key || '').toLowerCase()));
                    save(items().filter((h) => keep.has(h.key.toLowerCase())));
                }
            },
        };
        return list;
    };

    globalThis.__apifast_header_list = headerList;
})();
