// Original headless fixture: a Main marker proves whether the hook ran first.
if (Environment.GetEnvironmentVariable("ILLYA_FIXTURE_HOOK_REQUIRED") == "1"
    && !File.Exists(Environment.GetEnvironmentVariable("ILLYA_FIXTURE_HOOK_MARKER")))
    return 41;
File.WriteAllText(Environment.GetEnvironmentVariable("ILLYA_FIXTURE_MAIN_MARKER")!, "main");
return 23;
