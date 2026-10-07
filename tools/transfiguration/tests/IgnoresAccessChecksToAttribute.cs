namespace System.Runtime.CompilerServices;

// Test-only metadata for implementing the pinned API interface's internal scheduler member.
[AttributeUsage(AttributeTargets.Assembly, AllowMultiple = true)]
public sealed class IgnoresAccessChecksToAttribute(string assemblyName) : Attribute
{
    public string AssemblyName { get; } = assemblyName;
}
