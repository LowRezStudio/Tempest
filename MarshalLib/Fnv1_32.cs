using System.Text;

namespace MarshalLib;

internal static class Fnv1_32
{
    private const uint FnvOffsetBasis = 2166136261;
    private const uint FnvPrime = 16777619;

    public static uint ComputeHash(string s)
    {
        uint hash = FnvOffsetBasis;

        foreach (var c in Encoding.UTF8.GetBytes(s))
        {
            hash *= FnvPrime;
            hash ^= c;
        }

        return hash;
    }
}