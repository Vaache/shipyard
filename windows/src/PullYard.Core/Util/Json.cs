using System.Text.Json;
using System.Text.Json.Nodes;
using System.Text.Json.Serialization;

namespace PullYard.Core.Util;

public static class Json
{
    public static readonly JsonSerializerOptions Plain = new()
    {
        PropertyNameCaseInsensitive = true,
        PropertyNamingPolicy = JsonNamingPolicy.CamelCase,
        DefaultIgnoreCondition = JsonIgnoreCondition.WhenWritingNull
    };

    public static readonly JsonSerializerOptions GraphQL = Create(null);

    public static readonly JsonSerializerOptions Rest = Create(JsonNamingPolicy.SnakeCaseLower);

    private static JsonSerializerOptions Create(JsonNamingPolicy? policy)
    {
        var o = new JsonSerializerOptions
        {
            PropertyNameCaseInsensitive = true,
            PropertyNamingPolicy = policy,
            NumberHandling = JsonNumberHandling.AllowReadingFromString
        };
        o.Converters.Add(new NonNullListConverterFactory());
        return o;
    }

    public static string? Str(this JsonNode? node, string key) =>
        node is JsonObject o && o[key] is JsonValue v && v.TryGetValue<string>(out var s) ? s : null;

    public static bool? Bool(this JsonNode? node, string key) =>
        node is JsonObject o && o[key] is JsonValue v && v.TryGetValue<bool>(out var b) ? b : null;

    public static int? Int(this JsonNode? node, string key)
    {
        if (node is not JsonObject o || o[key] is not JsonValue v) return null;
        if (v.TryGetValue<int>(out var i)) return i;
        if (v.TryGetValue<long>(out var l)) return (int)l;
        if (v.TryGetValue<double>(out var d)) return (int)d;
        return null;
    }

    public static double? Dbl(this JsonNode? node, string key)
    {
        if (node is not JsonObject o || o[key] is not JsonValue v) return null;
        if (v.TryGetValue<double>(out var d)) return d;
        if (v.TryGetValue<long>(out var l)) return l;
        return null;
    }

    public static JsonObject? Obj(this JsonNode? node, string key) => node is JsonObject o ? o[key] as JsonObject : null;

    public static JsonArray? Arr(this JsonNode? node, string key) => node is JsonObject o ? o[key] as JsonArray : null;

    public static IEnumerable<JsonObject> Objects(this JsonArray? arr) => arr?.OfType<JsonObject>() ?? Enumerable.Empty<JsonObject>();

    public static IEnumerable<string> Strings(this JsonArray? arr)
    {
        if (arr is null) yield break;
        foreach (var n in arr)
            if (n is JsonValue v && v.TryGetValue<string>(out var s)) yield return s;
    }
}

public sealed class NonNullListConverterFactory : JsonConverterFactory
{
    public override bool CanConvert(Type typeToConvert) =>
        typeToConvert.IsGenericType && typeToConvert.GetGenericTypeDefinition() == typeof(List<>) && !typeToConvert.GetGenericArguments()[0].IsValueType;

    public override JsonConverter CreateConverter(Type typeToConvert, JsonSerializerOptions options)
    {
        var element = typeToConvert.GetGenericArguments()[0];
        return (JsonConverter)Activator.CreateInstance(typeof(NonNullListConverter<>).MakeGenericType(element))!;
    }

    private sealed class NonNullListConverter<T> : JsonConverter<List<T>> where T : class
    {
        public override List<T>? Read(ref Utf8JsonReader reader, Type typeToConvert, JsonSerializerOptions options)
        {
            if (reader.TokenType == JsonTokenType.Null) return new List<T>();
            if (reader.TokenType != JsonTokenType.StartArray) throw new JsonException("Expected array");
            var list = new List<T>();
            while (reader.Read())
            {
                if (reader.TokenType == JsonTokenType.EndArray) return list;
                if (reader.TokenType == JsonTokenType.Null) continue;
                var item = JsonSerializer.Deserialize<T>(ref reader, options);
                if (item is not null) list.Add(item);
            }
            throw new JsonException("Unterminated array");
        }

        public override void Write(Utf8JsonWriter writer, List<T> value, JsonSerializerOptions options)
        {
            writer.WriteStartArray();
            foreach (var item in value) JsonSerializer.Serialize(writer, item, options);
            writer.WriteEndArray();
        }
    }
}
