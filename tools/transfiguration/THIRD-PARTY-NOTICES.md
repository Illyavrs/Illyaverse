# Embedded dependency notices

The temporary connection DLL embeds the dependency-merged `Lib.Harmony` 2.4.2
assembly. The package includes the license texts in `licenses/`; distribute these
notices and that directory alongside launcher packages carrying the bridge.

| Component | Author / source | License |
| --- | --- | --- |
| Harmony 2.4.2 | [Andreas Pardeike](https://github.com/pardeike/Harmony/releases/tag/v2.4.2.0) | MIT, `Harmony.txt` |
| MonoMod components | [0x0ade and contributors](https://github.com/MonoMod/MonoMod) | MIT, `MonoMod.txt` |
| Mono.Cecil | [Jb Evain / Novell](https://github.com/jbevain/cecil) | MIT, `Mono.Cecil.txt` |
| Iced decoder components | [iced project and contributors](https://github.com/icedland/iced) | MIT, `Iced.txt` |
| .NET-derived compatibility code | [.NET Foundation and contributors](https://github.com/dotnet/runtime) | MIT, `DotNet.txt` |

The bridge resolves the player's existing osu!/framework/runtime assemblies
read-only and does not include them in its production package. Development proof
output contains its own NuGet-published runtime and is excluded from distribution.
No reference-project source is embedded or copied into this implementation.
