FROM mcr.microsoft.com/dotnet/sdk:10.0 AS build
WORKDIR /src

COPY Tempest.Protocol/Tempest.Protocol.csproj Tempest.Protocol/
COPY Tempest.Services/Tempest.Services.csproj Tempest.Services/
RUN dotnet restore Tempest.Services/Tempest.Services.csproj

COPY Tempest.Protocol/ Tempest.Protocol/
COPY Tempest.Services/ Tempest.Services/
RUN dotnet publish Tempest.Services/Tempest.Services.csproj -c Release -o /app/publish --no-restore

FROM mcr.microsoft.com/dotnet/aspnet:10.0
WORKDIR /app
COPY --from=build /app/publish .
EXPOSE 5197
EXPOSE 5198
ENTRYPOINT ["dotnet", "Tempest.Services.dll"]
