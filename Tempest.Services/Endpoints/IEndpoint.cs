namespace Tempest.Services.Endpoints;

/// <summary>
/// Marker interface for self-registering Minimal API endpoints.
/// Implementations are discovered via assembly scanning and registered by
/// <see cref="EndpointExtensions.AddEndpoints"/> / <see cref="EndpointExtensions.MapEndpoints"/>.
/// </summary>
public interface IEndpoint
{
    /// <summary>
    /// Maps this endpoint's routes onto the application's endpoint pipeline.
    /// </summary>
    static abstract void Map(IEndpointRouteBuilder app);
}
