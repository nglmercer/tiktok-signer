public static class OperatorPage
{
    public const string Html = """
        <!doctype html><html lang="en"><meta charset="utf-8"><meta name="viewport" content="width=device-width">
        <title>TikTok LIVE · VRChat relay</title>
        <style>body{font:17px system-ui;background:#111827;color:#e5e7eb;max-width:800px;margin:5vh auto;padding:24px}input,button{font:inherit;padding:12px;border-radius:8px;margin:6px 0}button{cursor:pointer}pre{white-space:pre-wrap;overflow-wrap:anywhere;background:#1f2937;padding:16px}input{width:60%}small{color:#cbd5e1}</style>
        <h1>TikTok LIVE → VRChat</h1><p>Enter the creator who is currently LIVE. The SDK runs on this relay; the world reads its finite JSON feed.</p>
        <form id="connect"><label for="creator">TikTok username</label><br><input id="creator" placeholder="@creator" maxlength="25" required><button>Connect</button></form>
        <button id="disconnect">Disconnect</button> <button id="fixtures">Load offline fixtures</button>
        <pre id="status" aria-live="polite">Loading…</pre>
        <p>World feed: <code id="url"></code></p><small>For other players or Quest, replace localhost with your deployed HTTPS relay URL. No signing bundle or authentication cookies are sent to the world.</small>
        <h2>Latest snapshot</h2><pre id="events"></pre>
        <script>
        const status = document.getElementById('status');
        document.getElementById('url').textContent = location.origin + '/vrchat/live';
        async function action(path, body={}) {
          try { const r=await fetch(path,{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify(body)});
            const data=await r.json(); status.textContent=JSON.stringify(data,null,2);
            if(!r.ok) return; await refresh();
          } catch(e) {status.textContent='Relay request failed: '+e.message;}
        }
        document.getElementById('connect').onsubmit=e=>{e.preventDefault();action('/operator/connect',{creator:document.getElementById('creator').value});};
        document.getElementById('disconnect').onclick=()=>action('/operator/disconnect');
        document.getElementById('fixtures').onclick=()=>action('/operator/fixtures');
        async function refresh(){try{const r=await fetch('/vrchat/live');if(!r.ok)throw Error('HTTP '+r.status);const d=await r.json();status.textContent=JSON.stringify(d.connection,null,2);document.getElementById('events').textContent=JSON.stringify(d.events,null,2);}catch(e){status.textContent=e.message;}}
        refresh();setInterval(refresh,5000);
        </script></html>
        """;
}
