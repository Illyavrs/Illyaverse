// Original offline ZIP fixture writer and installed archive-reader parity probe.
// No upstream archive-reader implementation is embedded in this program.
using System.Collections;
using System.Reflection;
using System.Reflection.Emit;
using System.Reflection.Metadata;
using System.Reflection.PortableExecutable;
using System.Runtime.InteropServices;
using System.Runtime.Loader;
using System.Security.Cryptography;
using System.Text;
using System.Text.Json;

internal static class Program
{
    private sealed record Fixture(string Id, byte[] RawName, ushort Flags, byte[] Extra, string[] Expected);
    private sealed record FixtureResult(string Id, string RawFilenameHex, int Flags, string ExtraHex,
        string ArchiveSha256, int ArchiveLength, string[] ExpectedFilenames, string[] ActualFilenames, bool MatchesExpectation);

    public static int Main(string[] args)
    {
        string stage = "validate_arguments_and_pins";
        try
        {
            if (args is ["--inspect-module", var assemblyFile])
            {
                inspectModule(Path.GetFullPath(assemblyFile));
                return 0;
            }
            if (args is ["--inspect-type", var dependencyFile, var typeName])
            {
                inspectModule(Path.GetFullPath(dependencyFile), typeName);
                return 0;
            }
            if (args.Length != 3) throw new ArgumentException("Arguments: installed client root, compatibility manifest, unique output directory");
            string nativeRoot = Path.GetFullPath(args[0]);
            string reportRoot = Path.GetFullPath(args[2]);
            if (!Directory.Exists(reportRoot) || !Guid.TryParseExact(Path.GetFileName(reportRoot), "N", out _))
                throw new ArgumentException("An existing UUID proof output directory is required");
            if (Directory.EnumerateFileSystemEntries(reportRoot, "report.json").Any())
                throw new IOException("This proof output already contains a report; choose a new UUID");

            using JsonDocument pins = JsonDocument.Parse(File.ReadAllText(args[1]));
            foreach (JsonProperty pin in pins.RootElement.GetProperty("assemblies").EnumerateObject())
            {
                string file = Path.Combine(nativeRoot, pin.Name);
                if (hashFile(file) != pin.Value.GetProperty("sha256").GetString()?.ToUpperInvariant())
                    throw new InvalidOperationException("Installed assembly does not match compatibility pin: " + pin.Name);
                if (AssemblyName.GetAssemblyName(file).Version?.ToString() != pin.Value.GetProperty("version").GetString())
                    throw new InvalidOperationException("Installed assembly version does not match compatibility pin: " + pin.Name);
            }

            // Only this archive class is used. The installed client entry point,
            // GameHost, APIAccess, storage and account constructors are never called.
            requireKnownGameInitializer(Path.Combine(nativeRoot, "osu.Game.dll"));
            stage = "load_game_assembly";
            AssemblyLoadContext.Default.Resolving += (_, name) => resolveDependency(nativeRoot, name);
            Assembly game = AssemblyLoadContext.Default.LoadFromAssemblyPath(Path.Combine(nativeRoot, "osu.Game.dll"));
            stage = "lookup_archive_reader_type";
            Type readerType = game.GetType("osu.Game.IO.Archives.ZipArchiveReader", throwOnError: true)!;
            stage = "lookup_archive_reader_members";
            ConstructorInfo constructor = readerType.GetConstructor([typeof(Stream), typeof(string)])
                ?? throw new MissingMethodException(readerType.FullName, ".ctor(Stream,string)");
            PropertyInfo filenames = readerType.GetProperty("Filenames")
                ?? throw new MissingMemberException(readerType.FullName, "Filenames");

            Fixture[] fixtures = createFixtures();
            var results = new List<FixtureResult>();
            foreach (Fixture fixture in fixtures)
            {
                stage = "enumerate_archive_fixture_" + fixture.Id;
                byte[] archive = writeStoredZip(fixture.RawName, fixture.Flags, fixture.Extra);
                if (archive.Length > 2048) throw new InvalidOperationException("Fixture size bound exceeded");
                string[] actual;
                using (var stream = new MemoryStream(archive, writable: false))
                using (var reader = (IDisposable)constructor.Invoke([stream, fixture.Id + ".osz"]))
                    actual = ((IEnumerable)filenames.GetValue(reader)!).Cast<string>().ToArray();
                if (actual.Length != 1) throw new InvalidOperationException("Expected one native filename in " + fixture.Id);
                var result = new FixtureResult(fixture.Id, Convert.ToHexString(fixture.RawName), fixture.Flags,
                    Convert.ToHexString(fixture.Extra), Convert.ToHexString(SHA256.HashData(archive)), archive.Length,
                    fixture.Expected, actual, fixture.Expected.SequenceEqual(actual, StringComparer.Ordinal));
                results.Add(result);
                Console.WriteLine(JsonSerializer.Serialize(result));
            }

            var nativeAssemblies = AssemblyLoadContext.Default.Assemblies
                .Where(a => !a.IsDynamic && !string.IsNullOrEmpty(a.Location)
                    && string.Equals(Path.GetDirectoryName(a.Location), nativeRoot, StringComparison.OrdinalIgnoreCase))
                .OrderBy(a => a.GetName().Name, StringComparer.Ordinal)
                .Select(a => new { Filename = Path.GetFileName(a.Location), Version = a.GetName().Version?.ToString(), Sha256 = hashFile(a.Location) })
                .ToArray();
            if (nativeAssemblies.Any(a => a.Filename == "osu!.dll"))
                throw new InvalidOperationException("Client executable assembly unexpectedly loaded");
            var report = new
            {
                Scope = "Installed ZipArchiveReader filename enumeration over bounded synthetic in-memory ZIPs only",
                ManagedInitialization = "Pinned game module initializer registers Realm type metadata; no Realm database instance or game/API/storage entry point is invoked",
                ProbeSha256 = hashFile(typeof(Program).Assembly.Location),
                NotVerified = new[] { "beatmap import", "play", "native GUI", "login", "network connectivity" },
                Runtime = RuntimeInformation.FrameworkDescription,
                RuntimeVersion = Environment.Version.ToString(),
                ClientPinnedRuntime = pins.RootElement.GetProperty("runtime").GetString(),
                ClientPinnedGameVersion = pins.RootElement.GetProperty("assemblies").GetProperty("osu.Game.dll").GetProperty("version").GetString(),
                NativeAssemblies = nativeAssemblies,
                FixtureCount = results.Count,
                MatchesExpectation = results.All(r => r.MatchesExpectation),
                Results = results,
            };
            File.WriteAllText(Path.Combine(reportRoot, "report.json"), JsonSerializer.Serialize(report, new JsonSerializerOptions { WriteIndented = true }));
            Console.WriteLine($"Archive-reader only: {results.Count} bounded fixtures; expected matches: {results.Count(r => r.MatchesExpectation)}");
            return results.All(r => r.MatchesExpectation) ? 0 : 2;
        }
        catch (Exception ex)
        {
            Console.Error.WriteLine("Stage " + stage + ": " + ex.GetBaseException().GetType().Name + ": " + ex.GetBaseException().Message);
            return 1;
        }
    }

    private static Assembly? resolveDependency(string nativeRoot, AssemblyName name)
    {
        // A closed allow-list prevents archive enumeration from reaching client
        // executable or additional unrelated native functionality.
        var allowed = new Dictionary<string, (string Version, string Sha256)>(StringComparer.Ordinal)
        {
            ["osu.Framework"] = ("2026.1005.0.0", "93B62F61C72539ED6FDB3C8EBA18D276A74DE8E1C094280B2531F5A76CAEA528"),
            ["Realm"] = ("20.1.0.0", "663844D062FAF837871F07C2630EF044942A317F150709F8E130038908D32241"),
            ["SharpCompress"] = ("0.50.4.0", "A2D92F04A0AD698CDC37A5BD27D52F2B763505BFEB9DF63802A7CBFBE313B25F"),
        };
        if (name.Name is null || !allowed.TryGetValue(name.Name, out var pin))
            throw new FileLoadException("Archive probe refused unapproved installed dependency: " + name.Name);
        string candidate = Path.Combine(nativeRoot, name.Name + ".dll");
        if (!File.Exists(candidate)) return null;
        if (hashFile(candidate) != pin.Sha256 || AssemblyName.GetAssemblyName(candidate).Version?.ToString() != pin.Version)
            throw new FileLoadException("Archive dependency does not match audited pin: " + name.Name);
        requireNoModuleInitializer(candidate);
        return AssemblyLoadContext.Default.LoadFromAssemblyPath(candidate);
    }

    private static void requireNoModuleInitializer(string file)
    {
        // Inspect PE metadata without loading or executing the assembly. A module
        // initializer would broaden the safe archive-only execution surface.
        using Stream stream = File.OpenRead(file);
        using var pe = new PEReader(stream);
        MetadataReader metadata = pe.GetMetadataReader();
        foreach (TypeDefinitionHandle handle in metadata.TypeDefinitions)
        {
            TypeDefinition type = metadata.GetTypeDefinition(handle);
            if (metadata.GetString(type.Name) != "<Module>") continue;
            if (type.GetMethods().Any(method => metadata.GetString(metadata.GetMethodDefinition(method).Name) == ".cctor"))
                throw new InvalidOperationException("Probe refuses module initialization in " + Path.GetFileName(file));
        }
    }

    private static void requireKnownGameInitializer(string file)
    {
        // The pinned PE was inspected without executing it. Its generated
        // initializer builds a Type[] and registers Realm schema metadata only.
        var expected = new Dictionary<string, string>(StringComparer.Ordinal)
        {
            [".<Module>..cctor"] = "E9E2C5BBC07974C0843E86A49B91ABAB7AAC1AD50B3B4694B5A1DAEE14D61DCD",
            [".RealmModuleInitializer.Initialize"] = "594121E34B3EC8912FF4381B12FF0662A9D15DF4DD632CAF00930B9F46C9C52A",
        };
        using Stream stream = File.OpenRead(file);
        using var pe = new PEReader(stream);
        MetadataReader metadata = pe.GetMetadataReader();
        foreach (MethodDefinitionHandle handle in metadata.MethodDefinitions)
        {
            string name = metadataName(metadata, handle);
            if (!expected.TryGetValue(name, out string? hash)) continue;
            byte[] il = pe.GetMethodBody(metadata.GetMethodDefinition(handle).RelativeVirtualAddress).GetILBytes()!;
            if (Convert.ToHexString(SHA256.HashData(il)) != hash)
                throw new InvalidOperationException("Game metadata initializer differs from audited body: " + name);
            expected.Remove(name);
        }
        if (expected.Count != 0) throw new InvalidOperationException("Audited game metadata initializer is missing");
    }

    private static void inspectModule(string file, string? requestedType = null)
    {
        using Stream stream = File.OpenRead(file);
        using var pe = new PEReader(stream);
        MetadataReader metadata = pe.GetMetadataReader();
        var opcodes = typeof(OpCodes).GetFields(BindingFlags.Public | BindingFlags.Static)
            .Where(f => f.FieldType == typeof(OpCode)).Select(f => (OpCode)f.GetValue(null)!)
            .ToDictionary(o => unchecked((ushort)o.Value));
        foreach (TypeDefinitionHandle typeHandle in metadata.TypeDefinitions)
        {
            TypeDefinition type = metadata.GetTypeDefinition(typeHandle);
            string currentType = metadata.GetString(type.Name);
            if (requestedType is null ? currentType is not ("<Module>" or "RealmModuleInitializer") : currentType != requestedType) continue;
            foreach (MethodDefinitionHandle methodHandle in type.GetMethods())
            {
                MethodDefinition method = metadata.GetMethodDefinition(methodHandle);
                if (metadata.GetString(method.Name) is not (".cctor" or "Initialize" or "AddDefaultTypes")) continue;
                byte[] il = pe.GetMethodBody(method.RelativeVirtualAddress).GetILBytes()!;
                Console.WriteLine(Path.GetFileName(file) + " " + metadataName(metadata, methodHandle) + " IL SHA256 " + Convert.ToHexString(SHA256.HashData(il)));
                for (int offset = 0; offset < il.Length;)
                {
                    int start = offset;
                    ushort value = il[offset++];
                    if (value == 0xfe) value = (ushort)(0xfe00 | il[offset++]);
                    OpCode opcode = opcodes[value];
                    int operandLength = opcode.OperandType switch
                    {
                        OperandType.InlineNone => 0,
                        OperandType.ShortInlineBrTarget or OperandType.ShortInlineI or OperandType.ShortInlineVar => 1,
                        OperandType.InlineVar => 2,
                        OperandType.InlineI8 or OperandType.InlineR => 8,
                        OperandType.InlineSwitch => 4 + 4 * BitConverter.ToInt32(il, offset),
                        _ => 4,
                    };
                    string operand = Convert.ToHexString(il.AsSpan(offset, operandLength));
                    if (opcode.OperandType is OperandType.InlineMethod or OperandType.InlineType)
                        operand += " " + metadataName(metadata, System.Reflection.Metadata.Ecma335.MetadataTokens.EntityHandle(BitConverter.ToInt32(il, offset)));
                    Console.WriteLine(start.ToString("X4") + " " + opcode.Name + " " + operand);
                    offset += operandLength;
                }
            }
        }
    }

    private static string metadataName(MetadataReader metadata, EntityHandle handle) => handle.Kind switch
    {
        HandleKind.MethodDefinition => metadataName(metadata, metadata.GetMethodDefinition((MethodDefinitionHandle)handle).GetDeclaringType())
            + "." + metadata.GetString(metadata.GetMethodDefinition((MethodDefinitionHandle)handle).Name),
        HandleKind.MemberReference => metadataName(metadata, metadata.GetMemberReference((MemberReferenceHandle)handle).Parent)
            + "." + metadata.GetString(metadata.GetMemberReference((MemberReferenceHandle)handle).Name),
        HandleKind.TypeReference => metadata.GetString(metadata.GetTypeReference((TypeReferenceHandle)handle).Namespace)
            + "." + metadata.GetString(metadata.GetTypeReference((TypeReferenceHandle)handle).Name),
        HandleKind.TypeDefinition => metadata.GetString(metadata.GetTypeDefinition((TypeDefinitionHandle)handle).Namespace)
            + "." + metadata.GetString(metadata.GetTypeDefinition((TypeDefinitionHandle)handle).Name),
        _ => handle.Kind.ToString(),
    };

    private static string hashFile(string file)
    {
        using Stream stream = File.OpenRead(file);
        return Convert.ToHexString(SHA256.HashData(stream));
    }

    private static Fixture[] createFixtures()
    {
        byte[] plain = Encoding.ASCII.GetBytes("plain.osu");
        byte[] legacy = Encoding.ASCII.GetBytes("legacy.osu");
        byte[] starUtf8 = Encoding.UTF8.GetBytes("星.osu");
        byte[] starCp932 = [0x90, 0xaf, 0x2e, 0x6f, 0x73, 0x75];
        return
        [
            new("ascii", plain, 0, [], ["plain.osu"]),
            new("utf8_japanese_flag", starUtf8, 0x0800, [], ["星.osu"]),
            new("cp932_so_trail_5c", [0x83, 0x5c, 0x2e, 0x6f, 0x73, 0x75], 0, [], ["ソ.osu"]),
            new("cp932_hyou_trail_5c", [0x95, 0x5c, 0x2e, 0x6f, 0x73, 0x75], 0, [], ["表.osu"]),
            new("cp932_star", starCp932, 0, [], ["星.osu"]),
            // Windows CP932 single-byte extensions; these expectations are
            // verified against the installed reader rather than WHATWG Shift_JIS.
            new("cp932_single_80", [0x80, 0x2e, 0x6f, 0x73, 0x75], 0, [], ["\u0080.osu"]),
            new("cp932_single_a0", [0xa0, 0x2e, 0x6f, 0x73, 0x75], 0, [], ["\uf8f0.osu"]),
            new("cp932_single_fd", [0xfd, 0x2e, 0x6f, 0x73, 0x75], 0, [], ["\uf8f1.osu"]),
            new("cp932_single_fe", [0xfe, 0x2e, 0x6f, 0x73, 0x75], 0, [], ["\uf8f2.osu"]),
            new("cp932_single_ff", [0xff, 0x2e, 0x6f, 0x73, 0x75], 0, [], ["\uf8f3.osu"]),
            new("cp932_trail_5c_in_folder", [.. Encoding.ASCII.GetBytes("folder/"), 0x83, 0x5c, 0x2e, 0x6f, 0x73, 0x75], 0, [], ["folder/ソ.osu"]),
            new("actual_backslash_separator", Encoding.ASCII.GetBytes("folder\\plain.osu"), 0, [], ["folder\\plain.osu"]),
            new("actual_backslash_before_cp932", [.. Encoding.ASCII.GetBytes("folder\\"), 0x83, 0x5c, 0x2e, 0x6f, 0x73, 0x75], 0, [], ["folder\\ソ.osu"]),
            new("unicode_path_valid_crc", legacy, 0, unicodePath(legacy, starUtf8, badCrc: false), ["星.osu"]),
            // Observed native behavior: this SharpCompress version uses the
            // Unicode Path field even when its filename CRC is incorrect.
            new("unicode_path_bad_crc", legacy, 0, unicodePath(legacy, starUtf8, badCrc: true), ["星.osu"]),
            new("invalid_utf8_flag", starCp932, 0x0800, [], ["��.osu"]),
            new("unicode_path_invalid_utf8", legacy, 0, unicodePath(legacy, [0xff, 0x2e, 0x6f, 0x73, 0x75], badCrc: false), ["�.osu"]),
        ];
    }

    private static byte[] unicodePath(byte[] rawName, byte[] utf8Name, bool badCrc)
    {
        using var output = new MemoryStream();
        using var writer = new BinaryWriter(output, Encoding.UTF8, leaveOpen: true);
        writer.Write((ushort)0x7075);
        writer.Write((ushort)(5 + utf8Name.Length));
        writer.Write((byte)1);
        writer.Write(crc32(rawName) ^ (badCrc ? 1u : 0u));
        writer.Write(utf8Name);
        return output.ToArray();
    }

    private static byte[] writeStoredZip(byte[] rawName, ushort flags, byte[] extra)
    {
        // One stored entry, fixed timestamp and tiny fixture content. Local and
        // central filename bytes/flags/extras are identical and bounded.
        byte[] content = "osu file format v14\n[General]\nMode: 0\n"u8.ToArray();
        if (rawName.Length is 0 or > 128 || extra.Length > 128) throw new ArgumentException("Fixture bound exceeded");
        uint crc = crc32(content);
        using var output = new MemoryStream();
        using var writer = new BinaryWriter(output, Encoding.UTF8, leaveOpen: true);
        writer.Write(0x04034b50u);
        writer.Write((ushort)20);
        writer.Write(flags);
        writer.Write((ushort)0);
        writer.Write((ushort)0);
        writer.Write((ushort)0x0021);
        writer.Write(crc);
        writer.Write((uint)content.Length);
        writer.Write((uint)content.Length);
        writer.Write((ushort)rawName.Length);
        writer.Write((ushort)extra.Length);
        writer.Write(rawName);
        writer.Write(extra);
        writer.Write(content);
        uint centralOffset = (uint)output.Position;
        writer.Write(0x02014b50u);
        writer.Write((ushort)20);
        writer.Write((ushort)20);
        writer.Write(flags);
        writer.Write((ushort)0);
        writer.Write((ushort)0);
        writer.Write((ushort)0x0021);
        writer.Write(crc);
        writer.Write((uint)content.Length);
        writer.Write((uint)content.Length);
        writer.Write((ushort)rawName.Length);
        writer.Write((ushort)extra.Length);
        writer.Write((ushort)0);
        writer.Write((ushort)0);
        writer.Write((ushort)0);
        writer.Write(0u);
        writer.Write(0u);
        writer.Write(rawName);
        writer.Write(extra);
        uint centralLength = (uint)output.Position - centralOffset;
        writer.Write(0x06054b50u);
        writer.Write((ushort)0);
        writer.Write((ushort)0);
        writer.Write((ushort)1);
        writer.Write((ushort)1);
        writer.Write(centralLength);
        writer.Write(centralOffset);
        writer.Write((ushort)0);
        return output.ToArray();
    }

    private static uint crc32(ReadOnlySpan<byte> bytes)
    {
        uint value = uint.MaxValue;
        foreach (byte b in bytes)
        {
            value ^= b;
            for (int bit = 0; bit < 8; bit++)
                value = (value >> 1) ^ (0xedb88320u & (0u - (value & 1u)));
        }
        return ~value;
    }
}
