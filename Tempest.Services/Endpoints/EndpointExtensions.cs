using System.Reflection;
using Microsoft.Extensions.DependencyInjection;

namespace Tempest.Services.Endpoints;

public static class EndpointExtensions
{
    /// <summary>
    /// Registers all <see cref="IEndpoint"/> implementations found in the given assemblies
    /// (defaults to the application's entry assembly) as transient services so they can
    /// also resolve scoped dependencies if needed.
    /// </summary>
    public static IServiceCollection AddEndpoints(this IServiceCollection services, params Assembly[] assemblies)
    {
        if (assemblies.Length == 0)
        {
            assemblies = [Assembly.GetEntryAssembly()!];
        }

        foreach (var assembly in assemblies)
        {
            var endpointTypes = assembly.GetTypes()
                .Where(t => t is { IsClass: true, IsAbstract: false, IsGenericTypeDefinition: false })
                .Where(t => typeof(IEndpoint).IsAssignableFrom(t));

            foreach (var type in endpointTypes)
            {
                services.AddTransient(type);
            }
        }

        return services;
    }

    /// <summary>
    /// Invokes <see cref="IEndpoint.Map"/> for every <see cref="IEndpoint"/> implementation
    /// found in the application's entry assembly. Call this once from the composition root,
    /// after authentication/authorization/routing middleware is in place.
    /// </summary>
    public static IEndpointRouteBuilder MapEndpoints(this IEndpointRouteBuilder app)
    {
        var assembly = Assembly.GetEntryAssembly()!;
        var endpointTypes = assembly.GetTypes()
            .Where(t => t is { IsClass: true, IsAbstract: false, IsGenericTypeDefinition: false })
            .Where(t => typeof(IEndpoint).IsAssignableFrom(t));

        foreach (var type in endpointTypes)
        {
            // IEndpoint.Map is static abstract — invoke via reflection without instantiation.
            var mapMethod = type.GetMethod(
                nameof(IEndpoint.Map),
                BindingFlags.Public | BindingFlags.Static);

            mapMethod?.Invoke(null, [app]);
        }

        return app;
    }
}
