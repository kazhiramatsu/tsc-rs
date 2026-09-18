async function main() {
    for (const d1 of [1, 2]) {
        try { await d1; } finally { const r = d1; if (r) await r; }
    }
}
