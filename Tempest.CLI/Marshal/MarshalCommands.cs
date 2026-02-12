using System.Text.Json;
using System.Text.Json.Serialization;
using MarshalLib;

namespace Tempest.CLI.Marshal;

internal partial class MarshalCommands
{
    /// <summary>Deserializes a marshal binary into JSON</summary>
    /// <param name="fields">Path of the exported fields.dat file</param>
    /// <param name="functions">Path of the exported functions.dat file</param>
    /// <param name="path">Export file path</param>
    /// <param name="output">Output file path, if not specified it's outputted to stdout</param>
    /// <param name="obscure">Required to parse the assembly, applies a 0x2A XOR</param>
    /// <param name="version">Marshal format version (Modern or Legacy)</param>
    public void Deserialize(string fields, string functions, string path, string? output = null, bool obscure = false, MarshalSerializerVersion version = MarshalSerializerVersion.Modern)
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
            using var outputStream = File.Open(output, FileMode.Create, FileAccess.Write, FileShare.None);

            JsonSerializer.Serialize(outputStream, result, MarshalSourceGenerationContext.Default.MarshalFunction);
        }
        else
        {
            JsonSerializer.Serialize(Console.OpenStandardOutput(), result, MarshalSourceGenerationContext.Default.MarshalFunction);
        }


        stream.Close();
    }

    /// <summary>Serializes JSON into a marshal binary</summary>
    /// <param name="fields">Path of the exported fields.dat file</param>
    /// <param name="functions">Path of the exported functions.dat file</param>
    /// <param name="path">JSON input file path</param>
    /// <param name="output">Output file path, if not specified it's outputted to stdout</param>
    /// <param name="version">Marshal format version (Modern or Legacy)</param>
    /// <param name="obscure">Applies a 0x2A XOR to the output</param>
    public void Serialize(string fields, string functions, string path, string? output = null, MarshalSerializerVersion version = MarshalSerializerVersion.Modern, bool obscure = false)
    {
        using var fieldsFile = File.OpenRead(fields);
        using var functionsFile = File.OpenRead(functions);

        var fieldMappings = FieldMappings.OpenRead(fieldsFile);
        var functionMappings = FunctionMappings.OpenRead(functionsFile);

        using var inputStream = File.OpenRead(path);
        var packet = JsonSerializer.Deserialize(inputStream, MarshalSourceGenerationContext.Default.MarshalFunction);

        if (packet == null)
            throw new Exception("Failed to deserialize marshal JSON.");

        var options = new MarshalSerializerOptions
        {
            FieldMappings = fieldMappings,
            FunctionMappings = functionMappings,
            Version = version
        };

        if (obscure)
        {
            using var buffer = new MemoryStream();
            MarshalSerializer.SerializeFunction(buffer, packet, options);

            var bytes = buffer.ToArray();
            for (var i = 0; i < bytes.Length; i++)
            {
                bytes[i] = (byte)(bytes[i] ^ 0x2A);
            }

            if (output != null)
            {
                using var outputStream = File.Open(output, FileMode.Create, FileAccess.Write, FileShare.None);
                outputStream.Write(bytes, 0, bytes.Length);
            }
            else
            {
                var stdout = Console.OpenStandardOutput();
                stdout.Write(bytes, 0, bytes.Length);
            }

            return;
        }

        if (output != null)
        {
            using var outputStream = File.Open(output, FileMode.Create, FileAccess.Write, FileShare.None);
            MarshalSerializer.SerializeFunction(outputStream, packet, options);
        }
        else
        {
            MarshalSerializer.SerializeFunction(Console.OpenStandardOutput(), packet, options);
        }
    }
}
