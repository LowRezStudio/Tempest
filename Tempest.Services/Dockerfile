FROM mcr.microsoft.com/dotnet/sdk:10.0-alpine AS build
WORKDIR /src
# ponytail: gcompat lets Grpc.Tools' linux_x64 protoc run on musl
RUN apk add --no-cache gcompat

COPY Tempest.Protocol/Tempest.Protocol.csproj Tempest.Protocol/
COPY Tempest.Services/Tempest.Services.csproj Tempest.Services/
RUN dotnet restore Tempest.Services/Tempest.Services.csproj

COPY Tempest.Protocol/ Tempest.Protocol/
COPY Tempest.Services/ Tempest.Services/
RUN dotnet publish Tempest.Services/Tempest.Services.csproj -c Release -o /app/publish --no-restore

FROM mcr.microsoft.com/dotnet/aspnet:10.0-alpine
WORKDIR /app
RUN apk add --no-cache curl
COPY --from=build /app/publish .
EXPOSE 80
ENV HTTP_PORTS=80
ENTRYPOINT ["dotnet", "Tempest.Services.dll"]
