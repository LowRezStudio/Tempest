using System.IO.Compression;
using Tempest.CLI.Extensions;

namespace Tempest.CLI.Marshal;

internal partial class MarshalCommands
{
    /// <summary>Extracts uncompressed function and field token data from a PE</summary>
    /// <param name="path">Path of the DLL or EXE</param>
    /// <param name="output">Output directory</param>
    public void ExtractTokens(string path, string output)
    {
        Directory.CreateDirectory(output);

        // Gzip magic only; the header's XFL/OS bytes differ between builds
        // (e.g. 04 00 vs 00 03), so match on 1F 8B 08 and verify by decoding.
        byte[] gzipHeader = [0x1F, 0x8B, 0x08];

        if (!File.Exists(path))
        {
            Console.Error.WriteLine("EXE/DLL was not found");
        }

        using var file = File.Open(path, FileMode.Open);
        var extractedCount = 0;

        long headerIndex;

        while ((headerIndex = file.IndexOfBytes(gzipHeader)) != -1)
        {
            file.Position = headerIndex;

            byte[] bytes;

            try
            {
                using var decompressionStream = new GZipStream(file, CompressionMode.Decompress, leaveOpen: true);
                using var memoryStream = new MemoryStream();
                decompressionStream.CopyTo(memoryStream);
                bytes = memoryStream.ToArray();
            }
            catch (InvalidDataException)
            {
                // False positive on the gzip magic; keep scanning.
                file.Position = headerIndex + 1;
                continue;
            }

            var filename = headerIndex.ToString();

            var helloIndex = bytes.IndexOfBytes("HELLO"u8.ToArray());
            var versionIndex = bytes.IndexOfBytes("VERSION"u8.ToArray());

            if (helloIndex != -1)
            {
                filename = "functions";
            }
            else if (versionIndex != -1)
            {
                filename = "fields";
            }

            File.WriteAllBytes(Path.Join(output, $"{filename}.dat"), bytes);

            Console.WriteLine($"Wrote all {bytes.Length} bytes to \"{filename}.dat\"");

            extractedCount++;

            file.Position = headerIndex + 1;
        }

        if (extractedCount == 0)
        {
            Console.Error.WriteLine("No token data found in the provided file.");
            Environment.ExitCode = 1;
        }
    }
}
