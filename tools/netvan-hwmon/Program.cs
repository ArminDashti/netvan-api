using System.Text.Json;
using System.Text.Json.Serialization;
using LibreHardwareMonitor.Hardware;

namespace NetvanHwmon;

internal static class Program
{
    [STAThread]
    private static int Main()
    {
        Console.InputEncoding = System.Text.Encoding.UTF8;
        Console.OutputEncoding = System.Text.Encoding.UTF8;

        var computer = new Computer
        {
            IsCpuEnabled = true,
            IsGpuEnabled = true,
            IsMemoryEnabled = true,
            IsMotherboardEnabled = true,
            IsControllerEnabled = true,
            IsStorageEnabled = true,
            IsNetworkEnabled = false,
            IsBatteryEnabled = true,
            IsPsuEnabled = true,
        };

        try
        {
            computer.Open();
        }
        catch
        {
            // Stay alive and return empty snapshots so the API never fails the RPC.
        }

        var visitor = new UpdateVisitor();
        var jsonOpts = new JsonSerializerOptions
        {
            DefaultIgnoreCondition = JsonIgnoreCondition.Never,
        };

        string? line;
        while ((line = Console.In.ReadLine()) != null)
        {
            if (line.Trim().Equals("quit", StringComparison.OrdinalIgnoreCase))
            {
                break;
            }

            var sensors = new List<SensorDto>();
            try
            {
                computer.Accept(visitor);
                Collect(computer.Hardware, sensors);
            }
            catch
            {
                sensors.Clear();
            }

            Console.Out.WriteLine(JsonSerializer.Serialize(new SnapshotDto { Sensors = sensors }, jsonOpts));
            Console.Out.Flush();
        }

        try
        {
            computer.Close();
        }
        catch
        {
            // ignore
        }

        return 0;
    }

    private static void Collect(IEnumerable<IHardware> hardwareList, List<SensorDto> outSensors)
    {
        foreach (var hw in hardwareList)
        {
            CollectOne(hw, outSensors);
            if (hw.SubHardware is { Length: > 0 })
            {
                Collect(hw.SubHardware, outSensors);
            }
        }
    }

    private static void CollectOne(IHardware hw, List<SensorDto> outSensors)
    {
        var kind = MapKind(hw.HardwareType);
        var hwName = string.IsNullOrWhiteSpace(hw.Name) ? hw.HardwareType.ToString() : hw.Name.Trim();
        foreach (var sensor in hw.Sensors)
        {
            if (sensor.SensorType != SensorType.Temperature)
            {
                continue;
            }

            var name = string.IsNullOrWhiteSpace(sensor.Name) ? "Temperature" : sensor.Name.Trim();
            outSensors.Add(new SensorDto
            {
                Id = sensor.Identifier.ToString(),
                HardwareKind = kind,
                HardwareName = hwName,
                SensorName = name,
                Celsius = sensor.Value.HasValue ? (double)sensor.Value.Value : null,
            });
        }
    }

    private static string MapKind(HardwareType type) => type switch
    {
        HardwareType.Cpu => "cpu",
        HardwareType.GpuNvidia => "gpu",
        HardwareType.GpuAmd => "gpu",
        HardwareType.GpuIntel => "gpu",
        HardwareType.Motherboard => "motherboard",
        HardwareType.SuperIO => "motherboard",
        HardwareType.EmbeddedController => "motherboard",
        HardwareType.Memory => "memory",
        HardwareType.Storage => "storage",
        _ => "other",
    };

    private sealed class UpdateVisitor : IVisitor
    {
        public void VisitComputer(IComputer computer) => computer.Traverse(this);

        public void VisitHardware(IHardware hardware)
        {
            hardware.Update();
            foreach (var sub in hardware.SubHardware)
            {
                sub.Accept(this);
            }
        }

        public void VisitSensor(ISensor sensor) { }

        public void VisitParameter(IParameter parameter) { }
    }

    private sealed class SnapshotDto
    {
        [JsonPropertyName("sensors")]
        public List<SensorDto> Sensors { get; set; } = [];
    }

    private sealed class SensorDto
    {
        [JsonPropertyName("id")]
        public string Id { get; set; } = "";

        [JsonPropertyName("hardware_kind")]
        public string HardwareKind { get; set; } = "other";

        [JsonPropertyName("hardware_name")]
        public string HardwareName { get; set; } = "";

        [JsonPropertyName("sensor_name")]
        public string SensorName { get; set; } = "";

        [JsonPropertyName("celsius")]
        public double? Celsius { get; set; }
    }
}
