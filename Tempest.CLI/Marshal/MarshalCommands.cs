using System.Text.Json;
using System.Text.Json.Serialization;
using MarshalLib;

namespace Tempest.CLI.Marshal;

internal partial class MarshalCommands
{
    /// <summary>Exports a marshal binary into another format</summary>
    /// <param name="fields">Path of the exported fields.dat file</param>
    /// <param name="functions">Path of the exported functions.dat file</param>
    /// <param name="path">Export file path</param>
    /// <param name="output">Output file path, if not specified it's outputted to stdout</param>
    /// <param name="obscure">Required to parse the assembly, applies a 0x2A XOR</param>
    /// <param name="version">Marshal format version (Modern or Legacy)</param>
    public void Export(string fields, string functions, string path, string? output = null, bool obscure = false, MarshalSerializerVersion version = MarshalSerializerVersion.Modern)
    {
        using var fieldsFile = File.OpenRead(fields);
        using var functionsFile = File.OpenRead(functions);

        var fieldMappings = FieldMappings.OpenRead(fieldsFile);
        var functionMappings = FunctionMappings.OpenRead(functionsFile);

        Stream stream;

        if (obscure)
        {
            var decoded = File.ReadAllBytes(path).Select(b => (byte)(b ^ 0x2A)).ToArray();

            stream = new MemoryStream(decoded);
        }
        else
        {
            stream = File.OpenRead(path);
        }

        var options = new MarshalSerializerOptions
        {
            FieldMappings = fieldMappings,
            FunctionMappings = functionMappings,
            Version = version
        };

        var result = MarshalSerializer.DeserializeFunction(stream, options);

        if (output != null)
        {
            using var outputStream = File.OpenWrite(output);

            JsonSerializer.Serialize(outputStream, result, MarshalSourceGenerationContext.Default.MarshalFunction);
        }
        else
        {
            JsonSerializer.Serialize(Console.OpenStandardOutput(), result, MarshalSourceGenerationContext.Default.MarshalFunction);
        }


        stream.Close();
    }
}