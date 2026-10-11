using UdonSharp;
using UnityEngine;
using TMPro;
using VRC.SDKBase;
using VRC.SDK3.Data;
using VRC.SDK3.StringLoading;

[UdonBehaviourSyncMode(BehaviourSyncMode.None)]
public class TikTokLivePanel : UdonSharpBehaviour
{
    public VRCUrl feedUrl;
    public TMP_Text statusText;
    public TMP_Text eventsText;
    public float pollSeconds = 6f;
    private bool _polling;
    private bool _pending;
    private bool _scheduled;
    private string[] _seen = new string[512];
    private int _seenIndex;
    private string _lines = "";
    private float _nextRequestAt;
    private string _streamId = "";
    private string _creator = "";

    private void Start()
    {
        if (statusText != null) statusText.richText = false;
        if (eventsText != null) eventsText.richText = false;
        StartPolling();
    }

    public void StartPolling()
    {
        if (_polling) return;
        _polling = true;
        if (!_scheduled && !_pending) Poll();
    }

    public void StopPolling()
    {
        _polling = false;
        if (statusText != null) statusText.text = "Polling paused. The relay connection is controlled on its webpage.";
    }

    public void Poll()
    {
        _scheduled = false;
        if (!_polling || _pending) return;
        if (Time.time < _nextRequestAt) { Schedule(); return; }
        if (VRCUrl.IsNullOrEmpty(feedUrl))
        {
            if (statusText != null) statusText.text = "Set the fixed relay feed URL in the Unity inspector.";
            return;
        }
        _pending = true;
        _nextRequestAt = Time.time + Mathf.Max(6f, pollSeconds);
        VRCStringDownloader.LoadUrl(feedUrl, this);
    }

    private void Schedule()
    {
        if (!_polling || _scheduled) return;
        _scheduled = true;
        SendCustomEventDelayedSeconds(nameof(Poll), Mathf.Max(6f, pollSeconds));
    }

    public override void OnStringLoadSuccess(IVRCStringDownload result)
    {
        _pending = false;
        if (!_polling) return;
        ApplySnapshot(result.Result);
        Schedule();
    }

    public override void OnStringLoadError(IVRCStringDownload result)
    {
        _pending = false;
        if (!_polling) return;
        if (statusText != null) statusText.text = "Relay HTTP error " + result.ErrorCode + ". Check relay URL and Allow Untrusted URLs.";
        Schedule();
    }

    // Also usable by the editor fixture validation without a live TikTok account.
    public void ApplySnapshot(string json)
    {
        DataToken root;
        if (json == null || json.Length > 131072 || !VRCJson.TryDeserializeFromJson(json, out root) || root.TokenType != TokenType.DataDictionary)
        {
            if (statusText != null) statusText.text = "Invalid or oversized relay JSON.";
            return;
        }
        DataToken connection;
        DataToken events;
        if (!root.DataDictionary.TryGetValue("connection", TokenType.DataDictionary, out connection) ||
            !root.DataDictionary.TryGetValue("events", TokenType.DataDictionary, out events)) return;
        string stream = Read(root.DataDictionary, "streamId");
        if (stream != _streamId)
        {
            _streamId = stream;
            _seen = new string[512];
            _seenIndex = 0;
            _lines = "";
        }
        var state = connection.DataDictionary;
        string creator = Read(state, "creator");
        if (creator != _creator) { _creator = creator; _lines = ""; }
        if (statusText != null) statusText.text = "@" + Read(state, "creator") + " · " + Read(state, "state") + "\n" + Read(state, "error");
        ReadEvents(events.DataDictionary, "chat", false);
        ReadEvents(events.DataDictionary, "gifts", true);
        if (eventsText != null) eventsText.text = _lines.Length == 0 ? "Waiting for chat or gifts…" : _lines;
    }

    private void ReadEvents(DataDictionary snapshot, string key, bool gifts)
    {
        DataToken entries;
        if (!snapshot.TryGetValue(key, TokenType.DataList, out entries)) return;
        DataList list = entries.DataList;
        int start = Mathf.Max(0, list.Count - 256);
        for (int i = start; i < list.Count; i++)
        {
            DataToken item = list[i];
            if (item.TokenType != TokenType.DataDictionary) continue;
            DataDictionary data = item.DataDictionary;
            string id = Read(data, "id");
            if (id.Length == 0 || Seen(id)) continue;
            _seen[_seenIndex] = id;
            _seenIndex = (_seenIndex + 1) % _seen.Length;
            string line = Read(data, "user") + ": ";
            if (gifts)
            {
                DataToken repeat;
                bool finished = data.TryGetValue("repeatEnd", TokenType.Boolean, out repeat) && repeat.Boolean;
                line += Read(data, "giftName") + " [gift " + Read(data, "giftId") + "] ×" + Read(data, "count") +
                    " (repeat " + Read(data, "repeatCount") + ", combo " + Read(data, "comboCount") + (finished ? ", complete)" : ")");
            }
            else line += Read(data, "text");
            _lines += line + "\n";
            if (_lines.Length > 6000)
            {
                int cut = _lines.IndexOf('\n', _lines.Length - 5000);
                _lines = cut >= 0 ? _lines.Substring(cut + 1) : _lines.Substring(_lines.Length - 5000);
            }
        }
    }

    private bool Seen(string id)
    {
        for (int i = 0; i < _seen.Length; i++) if (_seen[i] == id) return true;
        return false;
    }

    private string Read(DataDictionary data, string key)
    {
        DataToken token;
        if (!data.TryGetValue(key, out token)) return "";
        string value = token.TokenType == TokenType.String ? token.String : token.ToString();
        return value.Length > 250 ? value.Substring(0, 250) : value;
    }
}
