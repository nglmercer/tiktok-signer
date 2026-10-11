using System.IO;
using TMPro;
using UdonSharpEditor;
using UdonSharp;
using UnityEditor;
using UnityEditor.Events;
using UnityEditor.SceneManagement;
using UnityEngine;
using UnityEngine.SceneManagement;
using VRC.SDKBase;
using VRC.Udon;

[InitializeOnLoad]
public static class TikTokPanelSetup
{
    static TikTokPanelSetup() { EditorApplication.delayCall += InstallRequested; }
    private static void InstallRequested()
    {
        if (Application.isBatchMode || !File.Exists("Assets/TikTokBridge/install-request.txt") || EditorApplication.isPlayingOrWillChangePlaymode || EditorApplication.isCompiling) return;
        var scene = SceneManager.GetActiveScene();
        if (scene.path != "Assets/Scenes/StarterWorld.unity" || scene.isDirty)
        {
            Debug.LogWarning("TikTok panel installation pending: open/save StarterWorld, then run World Setup > Add TikTok LIVE Panel.");
            return;
        }
        Install();
    }

    [MenuItem("World Setup/Add TikTok LIVE Panel")]
    public static void Install()
    {
        var scene = SceneManager.GetActiveScene();
        if (!scene.IsValid() || scene.path.Length == 0) throw new System.Exception("Open a saved world scene first.");
        if (GameObject.Find("TikTok LIVE Panel") != null) { Complete(); return; }
        if (Resources.Load<TMP_Settings>("TMP Settings") == null)
        {
            TMP_PackageResourceImporter.ImportResources(true, false, false);
            AssetDatabase.Refresh(ImportAssetOptions.ForceSynchronousImport);
        }
        const string programPath = "Assets/TikTokBridge/TikTokLivePanel.asset";
        var program = AssetDatabase.LoadAssetAtPath<UdonSharpProgramAsset>(programPath);
        if (program == null)
        {
            program = ScriptableObject.CreateInstance<UdonSharpProgramAsset>();
            program.sourceCsScript = AssetDatabase.LoadAssetAtPath<MonoScript>("Assets/TikTokBridge/TikTokLivePanel.cs");
            AssetDatabase.CreateAsset(program, programPath);
            AssetDatabase.SaveAssets();
            AssetDatabase.Refresh(ImportAssetOptions.ForceSynchronousImport);
        }
        UdonSharp.Compiler.UdonSharpCompilerV1.CompileSync(new UdonSharp.Compiler.UdonSharpCompileOptions { IsEditorBuild = true });
        var welcome = GameObject.Find("Welcome Sign");
        if (welcome != null) Object.DestroyImmediate(welcome);
        var panel = new GameObject("TikTok LIVE Panel", typeof(RectTransform), typeof(Canvas),
            typeof(UnityEngine.UI.CanvasScaler), typeof(UnityEngine.UI.GraphicRaycaster));
        Undo.RegisterCreatedObjectUndo(panel, "Add TikTok LIVE Panel");
        var canvas = panel.GetComponent<Canvas>();
        canvas.renderMode = RenderMode.WorldSpace;
        var rect = panel.GetComponent<RectTransform>();
        rect.sizeDelta = new Vector2(1200, 850);
        rect.position = new Vector3(0, 2.2f, 5);
        rect.localScale = Vector3.one * 0.004f;
        panel.AddComponent<BoxCollider>().size = new Vector3(1200, 850, 1);
        foreach (var assembly in System.AppDomain.CurrentDomain.GetAssemblies())
        {
            var type = assembly.GetType("VRC.SDK3.Components.VRCUiShape");
            if (type != null) { panel.AddComponent(type); break; }
        }
        var background = panel.AddComponent<UnityEngine.UI.Image>();
        background.color = new Color(0.025f, 0.04f, 0.08f, 1f);
        background.raycastTarget = false;
        Text(panel.transform, "Heading", "TikTok LIVE · Activity Stage", new Vector2(0, 345), new Vector2(1080, 75), 42);
        Text(panel.transform, "Instructions", "Enter @creator on the relay webpage. This world polls its fixed feed URL every 6 seconds.", new Vector2(0, 260), new Vector2(1080, 90), 26);
        var status = Text(panel.transform, "Status", "Waiting for relay…", new Vector2(0, 155), new Vector2(1080, 100), 28);
        var events = Text(panel.transform, "AllLiveActivity", "Waiting for LIVE activity…", new Vector2(0, -70), new Vector2(1080, 320), 24);
        var behaviour = panel.AddUdonSharpComponent<TikTokLivePanel>();
        behaviour.feedUrl = new VRCUrl("http://127.0.0.1:15050/vrchat/live");
        behaviour.statusText = status;
        behaviour.eventsText = events;
        var beacon = GameObject.Find("LIVE Activity Beacon");
        if (beacon == null)
        {
            beacon = GameObject.CreatePrimitive(PrimitiveType.Sphere);
            beacon.name = "LIVE Activity Beacon";
            beacon.transform.position = new Vector3(3, 1.5f, 6);
            var material = new Material(Shader.Find("Standard"));
            material.color = new Color(0.1f, 0.85f, 0.9f);
            material.EnableKeyword("_EMISSION");
            material.SetColor("_EmissionColor", new Color(0.1f, 0.6f, 0.7f));
            const string materialPath = "Assets/TikTokBridge/ActivityBeacon.mat";
            if (!File.Exists(materialPath)) AssetDatabase.CreateAsset(material, materialPath);
            else Object.DestroyImmediate(material);
            beacon.GetComponent<Renderer>().sharedMaterial = AssetDatabase.LoadAssetAtPath<Material>(materialPath);
        }
        behaviour.activityBeacon = beacon.transform;
        UdonSharpEditorUtility.CopyProxyToUdon(behaviour);
        var backing = UdonSharpEditorUtility.GetBackingUdonBehaviour(behaviour);
        Button(panel.transform, "Start polling", new Vector2(-270, -330), backing, "StartPolling");
        Button(panel.transform, "Pause polling", new Vector2(270, -330), backing, "StopPolling");
        if (Object.FindObjectsOfType<UnityEngine.EventSystems.EventSystem>().Length == 0)
            new GameObject("EventSystem", typeof(UnityEngine.EventSystems.EventSystem), typeof(UnityEngine.EventSystems.StandaloneInputModule));
        EditorSceneManager.MarkSceneDirty(scene);
        EditorSceneManager.SaveScene(scene);
        AssetDatabase.SaveAssets();
        Complete();
        Debug.Log("TIKTOK_PANEL_INSTALLED: fixed URL, TMP labels and Udon buttons wired.");
    }

    private static double _deadline;
    public static void ImportAndVerifyAsync()
    {
        _deadline = EditorApplication.timeSinceStartup + 120;
        if (Resources.Load<TMP_Settings>("TMP Settings") == null)
            TMP_PackageResourceImporter.ImportResources(true, false, false);
        EditorApplication.update += WaitForResources;
    }
    private static void WaitForResources()
    {
        if (EditorApplication.isCompiling || EditorApplication.isUpdating) return;
        if (Resources.Load<TMP_Settings>("TMP Settings") == null)
        {
            if (EditorApplication.timeSinceStartup < _deadline) return;
            Debug.LogError("TMP resource import timed out.");
            EditorApplication.Exit(1); return;
        }
        EditorApplication.update -= WaitForResources;
        try { InstallAndVerify(); EditorApplication.Exit(0); }
        catch (System.Exception error) { Debug.LogException(error); EditorApplication.Exit(1); }
    }

    public static void UpgradeAndVerify()
    {
        EditorSceneManager.OpenScene("Assets/Scenes/StarterWorld.unity", OpenSceneMode.Single);
        var old = GameObject.Find("TikTok LIVE Panel");
        if (old != null) Object.DestroyImmediate(old);
        EditorSceneManager.SaveScene(SceneManager.GetActiveScene());
        InstallAndVerify();
    }

    public static void InstallAndVerify()
    {
        EditorSceneManager.OpenScene("Assets/Scenes/StarterWorld.unity", OpenSceneMode.Single);
        Install();
        UdonSharp.Compiler.UdonSharpCompilerV1.CompileSync(new UdonSharp.Compiler.UdonSharpCompileOptions { IsEditorBuild = true });
        var panel = GameObject.Find("TikTok LIVE Panel");
        if (panel == null || panel.GetComponent<UdonBehaviour>() == null)
            throw new System.Exception("TikTok panel or compiled Udon behaviour missing.");
        var proxy = panel.GetComponent<TikTokLivePanel>();
        if (proxy == null) proxy = (TikTokLivePanel)UdonSharpEditorUtility.GetProxyBehaviour(panel.GetComponent<UdonBehaviour>());
        if (proxy.statusText == null || proxy.eventsText == null || proxy.feedUrl == null || proxy.activityBeacon == null)
            throw new System.Exception("Panel references missing.");
        string json = File.ReadAllText("bridge-fixture.json");
        proxy.ApplySnapshot(json);
        string first = proxy.eventsText.text;
        proxy.ApplySnapshot(json);
        if (first != proxy.eventsText.text || !first.Contains("Rose") || !first.Contains("5655") || !first.Contains("FutureUnknownMessage"))
            throw new System.Exception("Fixture display/deduplication check failed.");
        if (proxy.statusText.richText || proxy.eventsText.richText)
            throw new System.Exception("Untrusted text must not use rich text.");
        Debug.Log("TIKTOK_BRIDGE_VERIFIED: fixture JSON parsed, all event categories displayed, duplicates suppressed.");
    }

    private static void Complete()
    {
        if (File.Exists("Assets/TikTokBridge/install-request.txt"))
            AssetDatabase.DeleteAsset("Assets/TikTokBridge/install-request.txt");
    }

    private static TextMeshProUGUI Text(Transform parent, string name, string value, Vector2 position, Vector2 size, int fontSize)
    {
        var item = new GameObject(name, typeof(RectTransform), typeof(TextMeshProUGUI));
        item.transform.SetParent(parent, false);
        var rect = item.GetComponent<RectTransform>();
        rect.anchoredPosition = position; rect.sizeDelta = size;
        var text = item.GetComponent<TextMeshProUGUI>();
        text.font = TMP_Settings.defaultFontAsset;
        text.text = value; text.fontSize = fontSize; text.color = Color.white;
        text.richText = false; text.raycastTarget = false;
        text.overflowMode = TextOverflowModes.Truncate;
        return text;
    }

    private static void Button(Transform parent, string title, Vector2 position, UdonBehaviour target, string eventName)
    {
        var item = new GameObject(title, typeof(RectTransform), typeof(UnityEngine.UI.Image), typeof(UnityEngine.UI.Button));
        item.transform.SetParent(parent, false);
        var rect = item.GetComponent<RectTransform>(); rect.anchoredPosition = position; rect.sizeDelta = new Vector2(460, 90);
        var image = item.GetComponent<UnityEngine.UI.Image>(); image.color = new Color(0.1f, 0.3f, 0.45f); image.raycastTarget = true;
        var button = item.GetComponent<UnityEngine.UI.Button>(); button.targetGraphic = image;
        Text(item.transform, "Label", title, Vector2.zero, new Vector2(440, 80), 30).alignment = TextAlignmentOptions.Center;
        UnityEventTools.AddStringPersistentListener(button.onClick, target.SendCustomEvent, eventName);
    }
}
